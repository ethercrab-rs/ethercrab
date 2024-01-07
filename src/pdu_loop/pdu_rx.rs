use super::storage::PduStorageRef;
use crate::{
    command::Command,
    error::{Error, PduError, PduValidationError},
    fmt,
    pdu_loop::{frame_header::FrameHeader, pdu_flags::PduFlags},
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
            return Ok(());
        }

        let i = raw_packet.payload();

        let header = FramePreamble::unpack_from_slice(i).map_err(|e| {
            fmt::error!("Failed to parse frame header: {}", e);

            e
        })?;

        let (data, working_counter) = header.data_wkc(i).map_err(|e| {
            fmt::error!("Could not get frame data/wkc: {}", e);

            e
        })?;

        let command = header.command()?;

        let FramePreamble {
            index, flags, irq, ..
        } = header;

        fmt::trace!(
            "Received frame with index {} ({:#04x}), WKC {}",
            index,
            index,
            working_counter,
        );

        let mut frame = self
            .storage
            .claim_receiving(index)
            .ok_or(PduError::InvalidIndex(index))?;

        if frame.index() != index {
            return Err(Error::Pdu(PduError::Validation(
                PduValidationError::IndexMismatch {
                    sent: frame.index(),
                    received: index,
                },
            )));
        }

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

        let frame_data = frame.buf_mut();

        frame_data[0..usize::from(flags.len())].copy_from_slice(data);

        frame.mark_received(flags, irq, working_counter)?;

        Ok(())
    }
}

/// PDU frame header, command, index, flags and IRQ.
#[derive(Copy, Clone, Hash, ethercrab_wire::EtherCrabWireRead)]
#[wire(bytes = 12)]
pub struct FramePreamble {
    #[wire(bytes = 2)]
    header: FrameHeader,

    // NOTE: The following fields are included in the header length field value.
    #[wire(bytes = 1)]
    command_code: u8,
    #[wire(bytes = 1)]
    index: u8,
    #[wire(bytes = 4)]
    command_raw: [u8; 4],
    #[wire(bytes = 2)]
    flags: PduFlags,
    #[wire(bytes = 2)]
    irq: u16,
}

impl FramePreamble {
    fn data_wkc<'buf>(&self, buf: &'buf [u8]) -> Result<(&'buf [u8], u16), Error> {
        // Jump past header in the buffer
        let header_offset = FramePreamble::PACKED_LEN;

        // The length of the PDU data body. There are two bytes after this that hold the working
        // counter, but are not counted as part of the PDU length from the header.
        let data_end = usize::from(self.header.payload_len);

        let data = buf.get(header_offset..data_end).ok_or(PduError::Decode)?;
        let wkc = buf
            .get(data_end..)
            .ok_or(Error::Pdu(PduError::Decode))
            .and_then(|raw| Ok(u16::unpack_from_slice(raw)?))?;

        Ok((data, wkc))
    }

    fn command(&self) -> Result<Command, Error> {
        Command::parse_code_data(self.command_code, self.command_raw)
    }
}
