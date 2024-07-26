use super::storage::PduStorageRef;
use crate::ethernet::{EthernetAddress, EthernetFrame};
use crate::{
    error::{Error, PduError},
    fmt,
    pdu_loop::frame_header::EthercatFrameHeader,
    ETHERCAT_ETHERTYPE, MASTER_ADDR,
};
use ethercrab_wire::{EtherCrabWireRead, EtherCrabWireSized};

/// EtherCAT frame receive adapter.
pub struct PduRx<'sto> {
    storage: PduStorageRef<'sto>,
    source_mac: EthernetAddress,
}

impl<'sto> PduRx<'sto> {
    pub(in crate::pdu_loop) fn new(storage: PduStorageRef<'sto>) -> Self {
        Self {
            storage,
            source_mac: MASTER_ADDR,
        }
    }

    /// Set the source MAC address to the given value.
    ///
    /// This is required on macOS (and BSD I believe) as the interface's MAC address cannot be
    /// overridden at the packet level for some reason.
    #[cfg(all(not(target_os = "linux"), unix))]
    pub(crate) fn set_source_mac(&mut self, new: EthernetAddress) {
        self.source_mac = new
    }

    /// Given a complete Ethernet II frame, parse a response PDU from it and wake the future that
    /// sent the frame.
    // NOTE: &mut self so this struct can only be used in one place.
    pub fn receive_frame(&mut self, ethernet_frame: &[u8]) -> Result<(), Error> {
        let raw_packet = EthernetFrame::new_checked(ethernet_frame)?;

        // Look for EtherCAT packets whilst ignoring broadcast packets sent from self. As per
        // <https://github.com/OpenEtherCATsociety/SOEM/issues/585#issuecomment-1013688786>, the
        // first SubDevice will set the second bit of the MSB of the MAC address (U/L bit). This means
        // if we send e.g. 10:10:10:10:10:10, we receive 12:10:10:10:10:10 which passes through this
        // filter.
        if raw_packet.ethertype() != ETHERCAT_ETHERTYPE || raw_packet.src_addr() == self.source_mac
        {
            fmt::trace!("Ignore frame");

            return Ok(());
        }

        let i = raw_packet.payload();

        let frame_header = EthercatFrameHeader::unpack_from_slice(i).map_err(|e| {
            fmt::error!("Failed to parse frame header: {}", e);

            e
        })?;

        // Skip EtherCAT header and get PDU(s) payload
        let i = i
            .get(
                EthercatFrameHeader::PACKED_LEN
                    ..(EthercatFrameHeader::PACKED_LEN + usize::from(frame_header.payload_len)),
            )
            .ok_or_else(|| {
                fmt::error!("Received frame is too short");

                Error::ReceiveFrame
            })?;

        // `i` now contains the EtherCAT frame payload, consisting of one or more PDUs including
        // their headers and payloads.

        // Second byte of first PDU header is the index
        let pdu_idx = *i.get(1).ok_or(Error::Internal)?;

        // We're assuming all PDUs in the returned frame have the same frame index, so we can just
        // use the first one.

        // PDU has its own EtherCAT index. This needs mapping back to the original frame.
        let frame_index = self
            .storage
            .frame_index_by_first_pdu_index(pdu_idx)
            .ok_or(Error::Pdu(PduError::Decode))?;

        fmt::trace!(
            "Receiving frame index {} (found from PDU {:#04x})",
            frame_index,
            pdu_idx
        );

        let mut frame = self
            .storage
            .claim_receiving(frame_index)
            .ok_or(PduError::InvalidIndex(frame_index))?;

        let frame_data = frame.buf_mut();

        frame_data
            .get_mut(0..i.len())
            .ok_or(Error::Internal)?
            .copy_from_slice(i);

        frame.mark_received()?;

        Ok(())
    }
}
