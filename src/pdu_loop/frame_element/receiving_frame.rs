use super::{received_frame::ReceivedFrame, FrameBox, PduMarker};
use crate::{
    error::{Error, PduError},
    fmt,
    pdu_loop::{
        frame_element::{FrameElement, FrameState},
        pdu_flags::PduFlags,
    },
};

use core::{
    future::Future,
    ptr::{addr_of_mut, NonNull},
    task::Poll,
};

/// A frame has been sent and is now waiting for a response from the network.
///
/// This state may only be entered once the frame has been sent over the network.
#[derive(Debug)]
pub struct ReceivingFrame<'sto> {
    pub inner: FrameBox<'sto>,
    pub pdu_states: &'sto [PduMarker],
}

impl<'sto> ReceivingFrame<'sto> {
    /// Mark the frame as fully received.
    ///
    /// This method may only be called once the frame response (header and data) has been validated
    /// and stored in the frame element.
    pub fn mark_received(
        &self,
        flags: PduFlags,
        irq: u16,
        working_counter: u16,
    ) -> Result<(), PduError> {
        unsafe { self.set_metadata(flags, irq, working_counter) };

        // Frame state must be updated BEFORE the waker is awoken so the future impl returns
        // `Poll::Ready`. The future will poll, see the `FrameState` as RxDone and return
        // Poll::Ready.
        unsafe {
            // NOTE: claim_receiving sets the state to `RxBusy` during parsing of the incoming frame
            // so the previous state here should be RxBusy.
            FrameElement::swap_state(self.inner.frame, FrameState::RxBusy, FrameState::RxDone)
                .map(|_| {})
                .map_err(|bad| {
                    fmt::error!(
                        "Failed to set frame {:#04x} state from RxBusy -> RxDone, got {:?}",
                        self.index(),
                        bad
                    );

                    PduError::InvalidFrameState
                })?;
        }

        // If the wake fails, release the receiving claim so the frame receive can possibly be
        // reattempted at a later time.
        if let Err(()) = unsafe { self.inner.wake() } {
            fmt::trace!("Failed to wake frame {:#04x}: no waker", self.index());

            unsafe {
                // Restore frame state to `Sent`, which is what `PduStorageRef::claim_receiving`
                // expects. This allows us to reprocess the frame again later. The frame will be
                // made reusable by `ReceiveFrameFut::drop` which sets the frame state to `None`,
                // preventing a deadlock of this PDU frame slot.
                //
                // If the frame is in another state, e.g. `RxProcessing` or other states that are
                // set after `RxDone`, the future is already being processed and likely doesn't even
                // need waking. In this case we can ignore the swap failure here.
                //
                // TODO: Only match on expected other states. Anything else should actually be a
                // logic bug.
                match FrameElement::swap_state(
                    self.inner.frame,
                    FrameState::RxDone,
                    FrameState::Sent,
                ) {
                    Ok(_) => (),
                    // Frame is being processed. We don't need to retry the receive
                    Err(bad_state)
                        if matches!(bad_state, FrameState::RxProcessing | FrameState::None) =>
                    {
                        fmt::trace!("--> Frame is {:?}, no need to wake", bad_state);

                        return Ok(());
                    }
                    Err(bad_state) => {
                        fmt::error!(
                            "Failed to set frame {:#04x} state from RxDone -> Sent, got {:?}",
                            self.index(),
                            bad_state
                        );

                        // Logic bug if the swap failed - no other threads should be using this
                        // frame, and the code just above this block sets the state to `RxDone`.
                        unreachable!();
                    }
                }
            }

            Err(PduError::NoWaker)
        } else {
            Ok(())
        }
    }

    unsafe fn set_metadata(&self, flags: PduFlags, irq: u16, working_counter: u16) {
        let frame = NonNull::new_unchecked(addr_of_mut!((*self.inner.frame.as_ptr()).frame));

        *addr_of_mut!((*frame.as_ptr()).flags) = flags;
        *addr_of_mut!((*frame.as_ptr()).irq) = irq;
        *addr_of_mut!((*frame.as_ptr()).working_counter) = working_counter;
    }

    pub fn buf_mut(&mut self) -> &mut [u8] {
        unsafe { self.inner.buf_mut() }
    }

    pub fn index(&self) -> u8 {
        unsafe { self.inner.frame() }.index
    }
}

pub struct ReceiveFrameFut<'sto> {
    pub(in crate::pdu_loop::frame_element) frame: Option<FrameBox<'sto>>,
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
        let rxin = match self.frame.take() {
            Some(r) => r,
            None => {
                fmt::error!("Frame is taken");

                return Poll::Ready(Err(PduError::InvalidFrameState.into()));
            }
        };

        unsafe { rxin.replace_waker(cx.waker()) };

        let frame_idx = unsafe { rxin.frame_index() };

        // RxDone is set by mark_received when the incoming packet has been parsed and stored
        let swappy = unsafe {
            FrameElement::swap_state(rxin.frame, FrameState::RxDone, FrameState::RxProcessing)
        };

        let was = match swappy {
            Ok(_frame_element) => {
                fmt::trace!("frame index {} is ready", frame_idx);

                return Poll::Ready(Ok(ReceivedFrame { inner: rxin }));
            }
            Err(e) => e,
        };

        fmt::trace!("frame index {} not ready yet ({:?})", frame_idx, was);

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
        if let Some(r) = self.frame.take() {
            fmt::debug!("Dropping in-flight future, possibly caused by timeout");

            r.release_pdu_claims();

            // Make frame available for reuse if this future is dropped.
            unsafe { FrameElement::set_state(r.frame, FrameState::None) };
        }
    }
}
