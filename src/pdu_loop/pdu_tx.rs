use super::{
    frame_element::{sendable_frame::SendableFrame, FrameBox, FrameElement},
    storage::PduStorageRef,
};
use crate::error::Error;
use core::{marker::PhantomData, ptr::NonNull, task::Waker};

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

    /// Iterate through any PDU TX frames that are ready and send them.
    ///
    /// The blocking `send` function is called for each ready frame. It is given a `SendableFrame`.
    pub fn send_frames_blocking<F>(&self, waker: &Waker, mut send: F) -> Result<(), Error>
    where
        F: FnMut(&SendableFrame<'_>) -> Result<(), ()>,
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

            match send(&sending) {
                Ok(_) => {
                    sending.mark_sent();
                }
                Err(_) => {
                    return Err(Error::SendFrame);
                }
            }
        }

        self.storage
            .tx_waker
            .try_write()
            .expect("update waker contention")
            .replace(waker.clone());

        Ok(())
    }
}
