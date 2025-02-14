use crate::{
    error::{Error, PduError},
    fmt,
    pdu_loop::frame_element::{received_frame::ReceivedFrame, FrameBox, FrameElement, FrameState},
    PduLoop,
};
use core::{future::Future, ptr::NonNull, sync::atomic::AtomicU8, task::Poll, time::Duration};
use futures_lite::FutureExt;

/// A frame has been sent and is now waiting for a response from the network.
///
/// This state may only be entered once the frame has been sent over the network.
#[derive(Debug)]
pub struct ReceivingFrame<'sto> {
    inner: FrameBox<'sto>,
}

impl<'sto> ReceivingFrame<'sto> {
    pub(in crate::pdu_loop) fn claim_receiving(
        frame: NonNull<FrameElement<0>>,
        pdu_idx: &'sto AtomicU8,
        frame_data_len: usize,
    ) -> Option<Self> {
        let frame = unsafe { FrameElement::claim_receiving(frame)? };

        Some(Self {
            inner: FrameBox::new(frame, pdu_idx, frame_data_len),
        })
    }

    /// Mark the frame as fully received.
    ///
    /// This method may only be called once the frame response (header and data) has been validated
    /// and stored in the frame element.
    pub(in crate::pdu_loop) fn mark_received(&self) -> Result<(), PduError> {
        // Frame state must be updated BEFORE the waker is awoken so the future impl returns
        // `Poll::Ready`. The future will poll, see the `FrameState` as RxDone and return
        // Poll::Ready.

        // NOTE: claim_receiving sets the state to `RxBusy` during parsing of the incoming frame
        // so the previous state here should be RxBusy.
        self.inner
            .swap_state(FrameState::RxBusy, FrameState::RxDone)
            .map_err(|bad| {
                fmt::error!(
                    "Failed to set frame {:#04x} state from RxBusy -> RxDone, got {:?}",
                    self.storage_slot_index(),
                    bad
                );

                PduError::InvalidFrameState
            })?;

        // wake() returns an error if there is no waker. A frame might have no waker if the response
        // is received over the network before the chosen executor has a chance to poll the future
        // for the first time, so we'll ignore that error otherwise we might get false positives.
        let _ = self.inner.wake();

        Ok(())
    }

    pub(in crate::pdu_loop) fn buf_mut(&mut self) -> &mut [u8] {
        self.inner.pdu_buf_mut()
    }

    /// Ethernet frame index.
    fn storage_slot_index(&self) -> u8 {
        self.inner.storage_slot_index()
    }
}

pub struct ReceiveFrameFut<'sto> {
    pub(in crate::pdu_loop::frame_element) frame: Option<FrameBox<'sto>>,
    pub(in crate::pdu_loop::frame_element) pdu_loop: &'sto PduLoop<'sto>,
    pub(in crate::pdu_loop::frame_element) timeout_timer: crate::timer_factory::Timer,
    pub(in crate::pdu_loop::frame_element) timeout: Duration,
    pub(in crate::pdu_loop::frame_element) retries_left: usize,
}

impl<'sto> ReceiveFrameFut<'sto> {
    /// Get entire frame buffer. Only really useful for assertions in tests.
    #[cfg(test)]
    pub fn buf(&self) -> &[u8] {
        use crate::{ethernet::EthernetFrame, pdu_loop::frame_header::EthercatFrameHeader};
        use ethercrab_wire::EtherCrabWireSized;

        let frame = self.frame.as_ref().unwrap();

        let b = frame.ethernet_frame();

        let len = EthernetFrame::<&[u8]>::buffer_len(frame.pdu_payload_len())
            + EthercatFrameHeader::PACKED_LEN;

        &b.into_inner()[0..len]
    }

    fn release(r: FrameBox<'sto>) {
        // Make frame available for reuse if this future is dropped.
        r.set_state(FrameState::None);
    }
}

// SAFETY: This unsafe impl is required due to `FrameBox` containing a `NonNull`, however this impl
// is ok because FrameBox also holds the lifetime `'sto` of the backing store, which is where the
// `NonNull<FrameElement>` comes from.
//
// For example, if the backing storage is is `'static`, we can send things between threads. If it's
// not, the associated lifetime will prevent the framebox from being used in anything that requires
// a 'static bound.
unsafe impl<'sto> Send for ReceiveFrameFut<'sto> {}

impl<'sto> Future for ReceiveFrameFut<'sto> {
    type Output = Result<ReceivedFrame<'sto>, Error>;

    fn poll(
        mut self: core::pin::Pin<&mut Self>,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<Self::Output> {
        let Some(rxin) = self.frame.take() else {
            fmt::error!("Frame is taken");

            return Poll::Ready(Err(PduError::InvalidFrameState.into()));
        };

        rxin.replace_waker(cx.waker());

        let frame_idx = rxin.storage_slot_index();

        // RxDone is set by mark_received when the incoming packet has been parsed and stored
        let swappy = rxin.swap_state(FrameState::RxDone, FrameState::RxProcessing);

        let was = match swappy {
            Ok(_) => {
                fmt::trace!("frame index {} is ready", frame_idx);

                return Poll::Ready(Ok(ReceivedFrame::new(rxin)));
            }
            Err(e) => e,
        };

        fmt::trace!("frame index {} not ready yet ({:?})", frame_idx, was);

        // Timeout checked after frame handling so we get at least one chance to receive reply from
        // network. This should mitigate race conditions when timeout expires just as the frame is
        // received.
        match self.timeout_timer.poll(cx) {
            Poll::Ready(_) => {
                // We timed out
                fmt::trace!(
                    "PDU response timeout with {} retries remaining",
                    self.retries_left
                );

                if self.retries_left == 0 {
                    // Release frame and PDU slots for reuse
                    Self::release(rxin);

                    return Poll::Ready(Err(Error::Timeout));
                }

                // If we have retry loops left:

                // Assign new timeout
                self.timeout_timer = crate::timer_factory::timer(self.timeout);
                // Poll timer once to register with the executor
                let _ = self.timeout_timer.poll(cx);

                // Mark frame as sendable once more
                rxin.set_state(FrameState::Sendable);
                // Wake frame sender so it picks up this frame we've just marked
                self.pdu_loop.wake_sender();

                self.retries_left -= 1;
            }
            Poll::Pending => {
                // Haven't timed out yet. Nothing to do - still waiting to be woken from the network
                // response.
            }
        }

        match was {
            FrameState::Sendable | FrameState::Sending | FrameState::Sent | FrameState::RxBusy => {
                self.frame = Some(rxin);

                Poll::Pending
            }
            state => {
                fmt::error!("Frame is in invalid state {:?}", state);

                Poll::Ready(Err(PduError::InvalidFrameState.into()))
            }
        }
    }
}

// If this impl is removed, timed out frames will never be reclaimed, clogging up the PDU loop and
// crashing the program.
impl<'sto> Drop for ReceiveFrameFut<'sto> {
    fn drop(&mut self) {
        // Frame option is taken when future completes successfully, so this drop logic will only
        // fire if the future is dropped before it completes.
        if let Some(r) = self.frame.take() {
            fmt::debug!("Dropping in-flight future, possibly caused by timeout");

            Self::release(r);
        }
    }
}
