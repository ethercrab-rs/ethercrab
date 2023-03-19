use super::{
    frame_element::{sendable_frame::SendableFrame, FrameBox, FrameElement},
    storage::PduStorageRef,
};
use crate::error::Error;
use core::{
    marker::PhantomData,
    ptr::NonNull,
    task::{Poll, Waker},
};

/// Send data frames over a network interface.
pub struct PduTx<'sto> {
    storage: PduStorageRef<'sto>,
}

// SAFETY: We're tied to the lifetime of the backing storage with 'sto.
unsafe impl<'sto> Send for PduTx<'sto> {}

impl<'sto> PduTx<'sto> {
    pub(in crate::pdu_loop) fn new(storage: PduStorageRef<'sto>) -> Self {
        Self { storage }
    }

    pub(crate) fn next_sendable_frame(&self) -> Option<SendableFrame<'sto>> {
        for idx in 0..self.storage.num_frames {
            let frame = unsafe { NonNull::new_unchecked(self.storage.frame_at_index(idx)) };

            let sending = if let Some(frame) = unsafe { FrameElement::claim_sending(frame) } {
                SendableFrame::new(FrameBox {
                    frame,
                    _lifetime: PhantomData,
                })
            } else {
                continue;
            };

            return Some(sending);
        }

        None
    }

    /// Iterate through any PDU TX frames that are ready and send them.
    ///
    /// The blocking `send` function is called for each ready frame.
    pub fn send_frames_blocking<F>(
        &self,
        waker: &Waker,
        packet_buf: &mut [u8],
        mut send: F,
    ) -> Result<(), Error>
    where
        F: FnMut(&[u8]) -> Result<(), ()>,
    {
        for idx in 0..self.storage.num_frames {
            let frame = unsafe { NonNull::new_unchecked(self.storage.frame_at_index(idx)) };

            let sending = if let Some(frame) = unsafe { FrameElement::claim_sending(frame) } {
                SendableFrame::new(FrameBox {
                    frame,
                    _lifetime: PhantomData,
                })
            } else {
                continue;
            };

            // FIXME: Release frame if it failed to write
            let packet = sending.write_ethernet_packet(packet_buf)?;

            match send(&packet) {
                Ok(_) => {
                    sending.mark_sent();
                }
                Err(_) => {
                    return Err(Error::SendFrame);
                }
            }
        }

        self.set_waker(waker.clone());

        Ok(())
    }

    pub(crate) fn set_waker(&self, waker: Waker) {
        let current_waker_guard = self.storage.tx_waker.upgradeable_read();

        if let Some(current_waker) = &*current_waker_guard {
            if !waker.will_wake(current_waker) {
                current_waker_guard.upgrade().replace(waker);
            }
        } else {
            current_waker_guard.upgrade().replace(waker);
        }
    }

    #[cfg_attr(windows, allow(unused))]
    pub(crate) fn next(&'sto self) -> PduTxFut<'sto> {
        PduTxFut { tx: self }
    }
}

pub struct PduTxFut<'sto> {
    tx: &'sto PduTx<'sto>,
}

impl<'sto> core::future::Future for PduTxFut<'sto> {
    type Output = Option<SendableFrame<'sto>>;

    fn poll(
        self: core::pin::Pin<&mut Self>,
        ctx: &mut core::task::Context<'_>,
    ) -> Poll<Self::Output> {
        match self.tx.next_sendable_frame() {
            Some(frame) => Poll::Ready(Some(frame)),
            None => {
                self.tx.set_waker(ctx.waker().clone());

                Poll::Pending
            }
        }
    }
}
