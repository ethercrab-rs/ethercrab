use crate::{command::Command, error::PduError, pdu_loop::frame_header::FrameHeader};
use cookie_factory::{
    bytes::{le_u16, le_u8},
    combinator::{skip, slice},
    gen_simple, GenError,
};
use packed_struct::prelude::*;

#[derive(Debug, Clone, Default)]
pub struct Pdu2<'a> {
    command: Command,
    index: u8,
    flags: PduFlags,
    irq: u16,
    data: &'a [u8],
    working_counter: u16,
}

const EMPTY_DATA: &[u8] = &[];

impl<'a> Pdu2<'a> {
    pub fn replace(
        &mut self,
        command: Command,
        data_length: u16,
        index: u8,
        data: &'a [u8],
    ) -> Result<(), PduError> {
        self.command = command;
        self.flags = PduFlags::with_len(data_length);
        self.irq = 0;
        self.index = index;

        self.data = data;

        // self.data
        //     .resize(usize::from(data_length), 0u8)
        //     .map_err(|_| PduError::TooLong)?;
        // self.data[0..data.len()].copy_from_slice(data);

        self.working_counter = 0;

        Ok(())
    }

    pub fn set_response(
        &mut self,
        flags: PduFlags,
        irq: u16,
        data: &'a [u8],
        working_counter: u16,
    ) -> Result<(), PduError> {
        self.flags = flags;
        self.irq = irq;

        // self.data
        //     .resize(data.len(), 0u8)
        //     .map_err(|_| PduError::TooLong)?;
        // self.data.copy_from_slice(data);

        self.data = data;

        self.working_counter = working_counter;

        Ok(())
    }

    pub fn nop() -> Self {
        Self {
            command: Command::Nop,
            index: 0,
            flags: PduFlags::with_len(0),
            irq: 0,
            data: EMPTY_DATA,
            working_counter: 0,
        }
    }

    /// The size of the total payload to be insterted into an EtherCAT frame.
    pub(crate) fn ethercat_payload_len(&self) -> usize {
        // TODO: Add unit test to stop regressions
        let pdu_overhead = 12;

        // NOTE: Sometimes data length is zero (e.g. for read-only ops), so we'll look at the actual
        // packet length in flags instead.
        usize::from(self.flags.len()) + pdu_overhead
    }

    /// Write an EtherCAT frame into `buf`.
    pub fn to_ethernet_payload<'buf>(&self, buf: &'buf mut [u8]) -> Result<(), PduError> {
        let header = FrameHeader::pdu(self.ethercat_payload_len());

        let buf = gen_simple(le_u16(header.0), buf).map_err(PduError::Encode)?;

        let buf = gen_simple(le_u8(self.command.code() as u8), buf)?;
        let buf = gen_simple(le_u8(self.index), buf)?;

        // Write address and register data
        let buf = gen_simple(slice(self.command.address()?), buf)?;

        let buf = gen_simple(le_u16(u16::from_le_bytes(self.flags.pack().unwrap())), buf)?;
        let buf = gen_simple(le_u16(self.irq), buf)?;

        // Probably a read; the sent packet's data area can be any old garbage, so we'll skip over it.
        // TODO: Read/write flag/enum to signal this more explicitly? "Probably" is a poor word to use...
        let buf = if self.data.is_empty() {
            gen_simple(skip(usize::from(self.flags.len())), buf)?
        }
        // Probably a write
        else {
            gen_simple(slice(&self.data), buf)?
        };

        // Working counter is always zero when sending
        let buf = gen_simple(le_u16(0u16), buf)?;

        if buf.len() != 0 {
            log::error!(
                "Expected fully used buffer, got {} bytes left instead",
                buf.len()
            );

            Err(PduError::Encode(GenError::BufferTooBig(buf.len())))
        } else {
            Ok(())
        }
    }

    pub fn index(&self) -> u8 {
        self.index
    }

    pub fn command(&self) -> Command {
        self.command
    }

    pub(crate) fn data(&self) -> &[u8] {
        self.data
    }

    pub(crate) fn working_counter(&self) -> u16 {
        self.working_counter
    }
}

#[derive(Default, Copy, Clone, Debug, PackedStruct, PartialEq, Eq)]
#[packed_struct(size_bytes = "2", bit_numbering = "msb0", endian = "lsb")]
pub struct PduFlags {
    /// Data length of this PDU.
    #[packed_field(bits = "0..=10")]
    pub(crate) length: u16,
    /// Circulating frame
    ///
    /// 0: Frame is not circulating,
    /// 1: Frame has circulated once
    #[packed_field(bits = "14")]
    circulated: bool,
    /// 0: last EtherCAT PDU in EtherCAT frame
    /// 1: EtherCAT PDU in EtherCAT frame follows
    #[packed_field(bits = "15")]
    is_not_last: bool,
}

impl PduFlags {
    pub const fn with_len(len: u16) -> Self {
        Self {
            length: len,
            circulated: false,
            is_not_last: false,
        }
    }

    pub const fn len(self) -> u16 {
        self.length
    }
}
