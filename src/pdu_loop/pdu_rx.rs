use super::storage::PduStorageRef;
use crate::{
    command::Command,
    error::{Error, PduError, PduValidationError},
    fmt,
    pdu_loop::{frame_header::FrameHeader, pdu_flags::PduFlags},
    ETHERCAT_ETHERTYPE, MASTER_ADDR,
};
use ethercrab_wire::EtherCrabWire;
use nom::{
    bytes::complete::take,
    combinator::map_res,
    error::context,
    number::complete::{le_u16, u8},
};
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
        // if we send e.g. 10:10:10:10:10:10, we receive 12:10:10:10:10:10 which is useful for this
        // filtering.
        if raw_packet.ethertype() != ETHERCAT_ETHERTYPE || raw_packet.src_addr() == self.source_mac
        {
            return Ok(());
        }

        let i = raw_packet.payload();

        let (i, header) = context("header", FrameHeader::parse)(i)?;

        // Only take as much as the header says we should
        let (_rest, i) = take(header.payload_len())(i)?;

        let (i, command_code) = u8(i)?;
        let (i, index) = u8(i)?;

        let mut frame = self
            .storage
            .claim_receiving(index)
            .ok_or_else(|| PduError::InvalidIndex(usize::from(index)))?;

        (|| {
            if frame.index() != index {
                return Err(Error::Pdu(PduError::Validation(
                    PduValidationError::IndexMismatch {
                        sent: frame.index(),
                        received: index,
                    },
                )));
            }

            let (i, command) = Command::parse(command_code, i)?;

            // Check for weird bugs where a slave might return a different command than the one sent for
            // this PDU index.
            if command.code() != frame.command().code() {
                return Err(Error::Pdu(PduError::Validation(
                    PduValidationError::CommandMismatch {
                        sent: command,
                        received: frame.command(),
                    },
                )));
            }

            let (i, flags) = map_res(take(2usize), PduFlags::unpack_from_slice)(i)?;
            let (i, irq) = le_u16(i)?;
            let (i, data) = take(flags.length)(i)?;
            let (i, working_counter) = le_u16(i)?;

            fmt::trace!(
                "Received frame with index {} ({:#04x}), WKC {}",
                index,
                index,
                working_counter,
            );

            // `_i` should be empty as we `take()`d an exact amount above.
            debug_assert_eq!(i.len(), 0, "trailing data in received frame");

            let frame_data = frame.buf_mut();

            frame_data[0..usize::from(flags.len())].copy_from_slice(data);

            frame.mark_received(flags, irq, working_counter)?;

            Ok(())
        })()
        .map_err(|e| {
            fmt::error!("Parse frame failed: {}", e);

            frame.release_receiving_claim();

            e
        })
    }
}
