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
use std::sync::Arc;

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

    pub(crate) fn next_sendable_frame(&self) -> Option<SendableFrame> {
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
        self.storage
            .tx_waker
            .try_write()
            .and_then(|mut writer| writer.replace(waker));
    }
}
