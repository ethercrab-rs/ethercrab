use crate::{
    command::{Command, CommandCode},
    frame::{FrameError, FrameHeader},
    LEN_MASK,
};
use cookie_factory::{
    bytes::{le_u16, le_u8},
    combinator::slice,
    gen_simple, GenError,
};
use core::mem;
use nom::{bytes::complete::take, combinator::map_res, IResult};
use packed_struct::prelude::*;

#[derive(Debug)]
pub struct Pdu<const MAX_DATA: usize> {
    command: Command,
    pub index: u8,
    flags: PduFlags,
    irq: u16,
    pub data: heapless::Vec<u8, MAX_DATA>,
    pub working_counter: u16,
}

impl<const MAX_DATA: usize> Pdu<MAX_DATA> {
    pub const fn new(command: Command, data_length: u16, index: u8) -> Self {
        debug_assert!(MAX_DATA <= LEN_MASK as usize);
        debug_assert!(data_length as usize <= MAX_DATA);

        Self {
            command,
            index,
            flags: PduFlags::with_len(data_length),
            irq: 0,
            data: heapless::Vec::new(),
            working_counter: 0,
        }
    }

    fn as_bytes<'a>(&self, buf: &'a mut [u8]) -> Result<&'a mut [u8], GenError> {
        // Order is VITAL here
        let buf = gen_simple(le_u8(self.command.code() as u8), buf)?;
        let buf = gen_simple(le_u8(self.index), buf)?;

        // Write address and register data
        let buf = gen_simple(slice(self.command.address()?), buf)?;

        let buf = gen_simple(le_u16(u16::from_le_bytes(self.flags.pack().unwrap())), buf)?;
        let buf = gen_simple(le_u16(self.irq), buf)?;
        let buf = gen_simple(slice(&self.data), buf)?;
        // Working counter is always zero when sending
        let buf = gen_simple(le_u16(0u16), buf)?;

        Ok(buf)
    }

    /// Compute the number of bytes required to store the PDU payload and metadata.
    const fn buf_len(&self) -> usize {
        // TODO: Add unit test to stop regressions
        MAX_DATA + 12
    }

    /// Compute the number of bytes required to store the PDU payload, metadata and EtherCAT frame
    /// header data.
    pub fn frame_buf_len(&self) -> usize {
        let size = self.buf_len() + mem::size_of::<FrameHeader>();

        // TODO: Move to unit test
        assert_eq!(size, MAX_DATA + 14);

        size
    }

    /// Write an EtherCAT PDU including frame header into the given buffer.
    pub fn write_ethernet_payload<'a>(&self, buf: &'a mut [u8]) -> Result<(), FrameError> {
        let header = FrameHeader::pdu(self.buf_len())?;

        let buf = gen_simple(le_u16(header.0), buf).map_err(FrameError::Encode)?;
        let _buf = self.as_bytes(buf).map_err(FrameError::Encode)?;

        Ok(())
    }

    /// Create an EtherCAT frame from an Ethernet II frame's payload.
    pub fn from_ethernet_payload<'a>(i: &[u8]) -> IResult<&[u8], Self> {
        // TODO: Split out frame header parsing when we want to support multiple PDUs. This should
        // also let us do better with the const generics.
        let (i, header) = FrameHeader::parse(i)?;

        // Only take as much as the header says we should
        let (_rest, i) = take(header.payload_len())(i)?;

        let (i, command_code) = map_res(nom::number::complete::u8, CommandCode::try_from)(i)?;
        let (i, index) = nom::number::complete::u8(i)?;
        let (i, command) = command_code.parse_address(i)?;
        let (i, flags) = map_res(take(2usize), PduFlags::unpack_from_slice)(i)?;
        let (i, irq) = nom::number::complete::le_u16(i)?;

        let (i, data) = map_res(take(flags.length), |slice: &[u8]| slice.try_into())(i)?;
        let (i, working_counter) = nom::number::complete::le_u16(i)?;

        Ok((
            i,
            Self {
                command,
                index,
                flags,
                irq,
                data,
                working_counter,
            },
        ))
    }

    // TODO: Proper error enum
    pub fn is_response_to(&self, request_pdu: &Self) -> Result<(), ()> {
        if request_pdu.index != self.index {
            return Err(());
        }

        if request_pdu.command != self.command {
            return Err(());
        }

        Ok(())
    }
}

#[derive(Copy, Clone, Debug, PackedStruct, PartialEq)]
#[packed_struct(size_bytes = "2", bit_numbering = "msb0", endian = "lsb")]
pub struct PduFlags {
    /// Data length of this PDU.
    #[packed_field(bits = "0..=10")]
    length: u16,
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
}
