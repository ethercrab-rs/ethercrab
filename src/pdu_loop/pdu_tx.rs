use super::{frame_element::sendable_frame::SendableFrame, storage::PduStorageRef};
use core::{sync::atomic::Ordering, task::Waker};

/// EtherCAT frame transmit adapter.
pub struct PduTx<'sto> {
    storage: PduStorageRef<'sto>,
}

impl<'sto> PduTx<'sto> {
    pub(in crate::pdu_loop) fn new(storage: PduStorageRef<'sto>) -> Self {
        Self { storage }
    }

    /// The number of frames that can be in flight at once.
    pub fn capacity(&self) -> usize {
        self.storage.num_frames
    }

    /// Get the next sendable frame, if any are available.
    // NOTE: Mutable so it can only be used in one task.
    pub fn next_sendable_frame(&mut self) -> Option<SendableFrame<'sto>> {
        for idx in 0..self.storage.num_frames {
            if self.should_exit() {
                return None;
            }

            let frame = self.storage.frame_at_index(idx);

            let Some(sending) = SendableFrame::claim_sending(
                frame,
                self.storage.pdu_idx,
                self.storage.frame_data_len,
            ) else {
                continue;
            };

            return Some(sending);
        }

        None
    }

    /// Set or replace the PDU loop waker.
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
    /// # static PDU_STORAGE: PduStorage<2, { PduStorage::element_size(2) }> = PduStorage::new();
    /// let (pdu_tx, _pdu_rx, _pdu_loop) = PDU_STORAGE.try_split().expect("can only split once");
    ///
    /// poll_fn(|ctx| {
    ///     // Set the waker so this future is polled again when new EtherCAT frames are ready to
    ///     // be sent.
    ///     pdu_tx.replace_waker(ctx.waker());
    ///
    ///     // Send and receive packets over the network interface here
    ///
    ///     Poll::<()>::Pending
    /// });
    /// ```
    #[cfg_attr(
        any(target_os = "windows", target_os = "macos", not(feature = "std")),
        allow(unused)
    )]
    pub fn replace_waker(&self, waker: &Waker) {
        self.storage.tx_waker.register(waker);
    }

    /// Returns `true` if the PDU sender should exit.
    ///
    /// This will be triggered by [`MainDevice::release_all`](crate::MainDevice::release_all). When
    /// giving back ownership of the `PduTx`, be sure to call [`release`](crate::PduTx::release) to
    /// ensure all internal state is correct before reuse.
    pub fn should_exit(&self) -> bool {
        self.storage.exit_flag.load(Ordering::Acquire)
    }

    /// Reset this object ready for reuse.
    pub fn release(self) -> Self {
        self.storage.exit_flag.store(false, Ordering::Relaxed);

        self
    }
}
