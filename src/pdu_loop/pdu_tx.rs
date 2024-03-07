use ethercrab_wire::{EtherCrabWireSized, EtherCrabWireWrite};
use smoltcp::wire::{EthernetAddress, EthernetFrame};

use super::{
    frame_element::{sendable_frame::SendableFrame, FrameBox, FrameElement},
    frame_header::FrameHeader,
    storage::PduStorageRef,
};
use crate::{
    error::{Error, PduError},
    ETHERCAT_ETHERTYPE, MASTER_ADDR,
};
use core::{marker::PhantomData, ptr::NonNull, task::Waker};

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

    fn has_sendable_frames(&mut self) -> bool {
        for idx in 0..self.storage.num_frames {
            let frame = unsafe { NonNull::new_unchecked(self.storage.frame_at_index(idx)) };

            if unsafe { FrameElement::<0>::is_sendable(frame) } {
                return true;
            }
        }

        false
    }

    pub(crate) fn pack_buffer<'buf>(
        &mut self,
        buf: &'buf mut [u8],
    ) -> Result<Option<&'buf [u8]>, Error> {
        let mut ethernet_frame =
            EthernetFrame::new_checked(buf).map_err(|_| PduError::CreateFrame)?;

        ethernet_frame.set_src_addr(MASTER_ADDR);
        ethernet_frame.set_dst_addr(EthernetAddress::BROADCAST);
        ethernet_frame.set_ethertype(ETHERCAT_ETHERTYPE);

        let ethernet_payload = ethernet_frame.payload_mut();

        let (mut ethercat_header_bytes, mut ethernet_payload) =
            ethernet_payload.split_at_mut(FrameHeader::PACKED_LEN);

        let mut total_len = 0;

        while let Some(mut frame) = self.next_sendable_frame() {
            let pdu_len = usize::from(frame.ethercat_payload_len());

            // Buffer is too short to hold another frame
            if pdu_len > ethernet_payload.len() {
                frame.release_sending_claim();

                break;
            }

            total_len += pdu_len;

            frame.set_more_follows(self.has_sendable_frames());

            // Write PDU only, no Ethernet header
            ethernet_payload = frame.write_pdu(ethernet_payload);

            frame.mark_sent();
        }

        if total_len == 0 {
            return Ok(None);
        }

        let ecat_header = FrameHeader::pdu(total_len as u16);
        ecat_header.pack_to_slice_unchecked(&mut ethercat_header_bytes);

        let total_len = EthernetFrame::<&[u8]>::buffer_len(total_len + FrameHeader::PACKED_LEN);

        Ok(Some(&ethernet_frame.into_inner()[0..total_len]))
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
    /// # static PDU_STORAGE: PduStorage<2, 2> = PduStorage::new();
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
}
