use crate::{
    command::{Command, CommandCode},
    error::{Error, PduError, PduValidationError},
    pdu_loop::frame_header::FrameHeader,
    ETHERCAT_ETHERTYPE, LEN_MASK, MASTER_ADDR,
};
use cookie_factory::{
    bytes::{le_u16, le_u8},
    combinator::slice,
    gen_simple, GenError,
};
use core::mem;
use nom::{
    bytes::complete::take,
    combinator::map_res,
    error::{context, ContextError, FromExternalError, ParseError},
    IResult,
};
use num_enum::TryFromPrimitiveError;
use packed_struct::prelude::*;
use smoltcp::wire::{EthernetAddress, EthernetFrame};

pub type PduResponse<T> = (T, u16);

pub trait CheckWorkingCounter<T> {
    fn wkc(self, expected: u16, context: &'static str) -> Result<T, Error>;
}

impl<T> CheckWorkingCounter<T> for PduResponse<T> {
    fn wkc(self, expected: u16, context: &'static str) -> Result<T, Error> {
        if self.1 == expected {
            Ok(self.0)
        } else {
            Err(Error::WorkingCounter {
                expected,
                received: self.1,
                context: Some(context),
            })
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Pdu<const MAX_DATA: usize> {
    command: Command,
    index: u8,
    flags: PduFlags,
    irq: u16,
    data: heapless::Vec<u8, MAX_DATA>,
    working_counter: u16,
}

impl<const MAX_DATA: usize> Pdu<MAX_DATA> {
    pub fn new(
        command: Command,
        data_length: u16,
        index: u8,
        data: &[u8],
    ) -> Result<Self, PduError> {
        debug_assert!(MAX_DATA <= LEN_MASK as usize);
        debug_assert!(data_length as usize <= MAX_DATA);

        // TODO: Is there a way I can do this without copying/cloning?
        let data = heapless::Vec::from_slice(data).map_err(|_| PduError::TooLong)?;

        Ok(Self {
            command,
            index,
            flags: PduFlags::with_len(data_length),
            irq: 0,
            data,
            working_counter: 0,
        })
    }

    pub fn nop() -> Self {
        Self {
            command: Command::Nop,
            index: 0,
            flags: PduFlags::with_len(0),
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

    /// Write an ethernet frame into `buf`, returning the used portion of the buffer.
    pub fn to_ethernet_frame<'a>(&self, buf: &'a mut [u8]) -> Result<&'a [u8], PduError> {
        let ethernet_len = EthernetFrame::<&[u8]>::buffer_len(self.frame_buf_len());

        let buf = buf.get_mut(0..ethernet_len).ok_or(PduError::TooLong)?;

        let mut ethernet_frame = EthernetFrame::new_checked(buf).map_err(PduError::CreateFrame)?;

        ethernet_frame.set_src_addr(MASTER_ADDR);
        ethernet_frame.set_dst_addr(EthernetAddress::BROADCAST);
        ethernet_frame.set_ethertype(ETHERCAT_ETHERTYPE);

        let header = FrameHeader::pdu(self.buf_len());

        let buf = ethernet_frame.payload_mut();

        let buf = gen_simple(le_u16(header.0), buf).map_err(PduError::Encode)?;
        let _buf = self.as_bytes(buf).map_err(PduError::Encode)?;

        let buf = ethernet_frame.into_inner();

        Ok(buf)
    }

    /// Create an EtherCAT frame from an Ethernet II frame's payload.
    pub fn from_ethernet_payload<'a, E>(i: &'a [u8]) -> IResult<&'a [u8], Self, E>
    where
        E: ParseError<&'a [u8]>
            + ContextError<&'a [u8]>
            + FromExternalError<&'a [u8], TryFromPrimitiveError<CommandCode>>
            + FromExternalError<&'a [u8], PackingError>
            + FromExternalError<&'a [u8], ()>,
    {
        // TODO: Split out frame header parsing when we want to support multiple PDUs. This should
        // also let us do better with the const generics.
        let (i, header) = context("header", FrameHeader::parse)(i)?;

        // Only take as much as the header says we should
        let (_rest, i) = context("take", take(header.payload_len()))(i)?;

        let (i, command_code) = context(
            "command code",
            map_res(nom::number::complete::u8, CommandCode::try_from),
        )(i)?;
        let (i, index) = context("index", nom::number::complete::u8)(i)?;
        let (i, command) = context("command", |i| command_code.parse_address(i))(i)?;
        let (i, flags) = context("flags", map_res(take(2usize), PduFlags::unpack_from_slice))(i)?;
        let (i, irq) = context("irq", nom::number::complete::le_u16)(i)?;

        let (i, data) = context(
            "data",
            map_res(take(flags.length), |slice: &[u8]| slice.try_into()),
        )(i)?;
        let (i, working_counter) = context("working counter", nom::number::complete::le_u16)(i)?;

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

    pub fn is_response_to(&self, request_pdu: &Self) -> Result<(), PduValidationError> {
        if request_pdu.index != self.index {
            return Err(PduValidationError::IndexMismatch {
                sent: request_pdu.command,
                received: self.command,
            });
        }

        if request_pdu.command.code() != self.command.code() {
            return Err(PduValidationError::CommandMismatch {
                sent: request_pdu.command,
                received: self.command,
            });
        }

        Ok(())
    }

    pub fn index(&self) -> u8 {
        self.index
    }

    pub(crate) fn data(&self) -> &[u8] {
        self.data.as_slice()
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
