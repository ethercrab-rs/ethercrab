use super::{
    frame_element::{sendable_frame::SendableFrame, FrameBox, FrameElement},
    storage::PduStorageRef,
};
use core::{marker::PhantomData, ptr::NonNull, task::Waker};
use spin::RwLockWriteGuard;

/// EtherCAT frame transmit adapter.
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

    /// Get a handle to the internal PDU loop waker.
    ///
    /// The waker must be set otherwise the future in charge of sending new packets will not be
    /// woken again, causing a timeout error.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use ethercrab::PduStorage;
    /// use core::future::poll_fn;
    /// use core::task::Poll;
    ///
    /// # static PDU_STORAGE: PduStorage<2, 2> = PduStorage::new();
    /// let (pdu_tx, _pdu_rx, _pdu_loop) = PDU_STORAGE.try_split().expect("can only split once");
    ///
    /// poll_fn(|ctx| {
    ///     // Send and receive packets over the network interface here
    ///
    ///     // Set the waker so this future is polled again when new EtherCAT frames are ready to
    ///     // be sent.
    ///     pdu_tx.waker().replace(ctx.waker().clone());
    ///
    ///     Poll::<()>::Pending
    /// });
    /// ```
    #[cfg_attr(
        any(target_os = "windows", target_os = "macos", not(feature = "std")),
        allow(unused)
    )]
    pub fn waker<'lock>(&self) -> RwLockWriteGuard<'lock, Option<Waker>>
    where
        'sto: 'lock,
    {
        self.storage.tx_waker.write()
    }
}
