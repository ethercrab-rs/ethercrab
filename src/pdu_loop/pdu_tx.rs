use super::{
    frame_element::{sendable_frame::SendableFrame, FrameBox, FrameElement},
    storage::PduStorageRef,
};
use core::{marker::PhantomData, ptr::NonNull, task::Waker};
use spin::RwLockWriteGuard;

/// Send data frames over a network interface.
pub struct PduTx<'sto> {
    storage: PduStorageRef<'sto>,
}

// SAFETY: We're tied to the lifetime of the backing storage with 'sto.
unsafe impl<'sto> Send for PduTx<'sto> {}
unsafe impl<'sto> Sync for PduTx<'sto> {}

impl<'sto> PduTx<'sto> {
    pub(in crate::pdu_loop) fn new(storage: PduStorageRef<'sto>) -> Self {
        Self { storage }
    }

    /// Get the next sendable frame, if any are available.
    // NOTE: Mutable so it can only be used in one task.
    pub fn next_sendable_frame(&mut self) -> Option<SendableFrame<'sto>> {
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

    #[cfg_attr(windows, allow(unused))]
    pub(crate) fn lock_waker<'lock>(&self) -> RwLockWriteGuard<'lock, Option<Waker>>
    where
        'sto: 'lock,
    {
        self.storage.tx_waker.write()
    }
}
