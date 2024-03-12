use super::storage::PduStorageRef;
use crate::{
    error::{Error, PduError},
    fmt,
    pdu_loop::{frame_header::EthercatFrameHeader, pdu_header::PduHeader, PDU_UNUSED_SENTINEL},
    ETHERCAT_ETHERTYPE, MASTER_ADDR,
};
use ethercrab_wire::{EtherCrabWireRead, EtherCrabWireSized};
use smoltcp::wire::{EthernetAddress, EthernetFrame};

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
        // first slave will set the second bit of the MSB of the MAC address (U/L bit). This means
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

        let i = i
            // Strip EtherCAT frame header...
            .get(EthercatFrameHeader::PACKED_LEN..)
            // ...then take as much as we're supposed to
            .and_then(|i| i.get(0..usize::from(frame_header.payload_len)))
            .ok_or_else(|| {
                fmt::error!("Received frame is too short");

                Error::ReceiveFrame
            })?;

        // `i` now contains the EtherCAT frame payload, consisting of one or more PDUs including
        // header and payload.

        // Only support a single PDU per frame for now
        let pdu_header = PduHeader::unpack_from_slice(i)?;

        // PDU has its own EtherCAT index. This needs mapping back to the original frame.
        // TODO: Put this in a method that checks for sentinel. Should also take a `&PduHeader` for
        // more type safety.
        let frame_index = self.storage.pdu_states[usize::from(pdu_header.index)].frame_index();

        fmt::trace!("Received frame index {}", frame_index);

        let (data, working_counter) = pdu_header.data_wkc(i).map_err(|e| {
            fmt::error!("Could not get frame data/wkc: {}", e);

            e
        })?;

        let command = pdu_header.command()?;

        let PduHeader {
            index: pdu_idx,
            flags,
            irq,
            ..
        } = pdu_header;

        fmt::trace!("--> Received PDU {:#04x}, WKC {}", pdu_idx, working_counter,);

        let mut frame = self
            .storage
            .claim_receiving(frame_index)
            .ok_or(PduError::InvalidIndex(frame_index))?;

        // TODO: Validate received PDU against what was sent. (command, PDU index, maybe length?).

        // if frame.index() != index {
        //     return Err(Error::Pdu(PduError::Validation(
        //         PduValidationError::IndexMismatch {
        //             sent: frame.index(),
        //             received: index,
        //         },
        //     )));
        // }

        // // Check for weird bugs where a slave might return a different command than the one sent for
        // // this PDU index.
        // if command.code() != frame.command().code() {
        //     return Err(Error::Pdu(PduError::Validation(
        //         PduValidationError::CommandMismatch {
        //             sent: command,
        //             received: frame.command(),
        //         },
        //     )));
        // }

        let frame_data = frame.buf_mut();

        frame_data[0..usize::from(flags.len())].copy_from_slice(data);

        frame.mark_received(flags, irq, working_counter)?;

        Ok(())
    }
}
