use crate::LEN_MASK;
use cookie_factory::{
    bytes::{le_i16, le_u16, le_u32, le_u8},
    combinator::slice,
    gen_simple, GenError,
};
use core::mem;
use nom::{
    bytes::complete::take,
    combinator::{map, map_res, verify},
    error::ErrorKind,
    IResult,
};
use packed_struct::{prelude::*, types::bits::Bits, PackedStructInfo};
use smoltcp::wire::EthernetFrame;

// TODO: Logical PDU with 32 bit address
// TODO: Auto increment PDU with i16 address
#[derive(Debug)]
pub struct Pdu<const N: usize> {
    command: Command,
    index: u8,
    register_address: u16,
    flags: PduFlags,
    irq: u16,
    data: [u8; N],
    working_counter: u16,
}

impl<const N: usize> Pdu<N> {
    pub const fn brd(register_address: u16) -> Self {
        debug_assert!(N < LEN_MASK as usize);

        Self {
            // Start at master address 0
            command: Command::Brd { address: 0 },
            index: 0,
            register_address,
            flags: PduFlags::with_len(N as u16),
            irq: 0,
            data: [0u8; N],
            working_counter: 0,
        }
    }

    fn as_bytes<'a>(&self, buf: &'a mut [u8]) -> Result<&'a mut [u8], GenError> {
        // Order is VITAL here
        let buf = gen_simple(le_u8(self.command.code() as u8), buf)?;
        let buf = gen_simple(le_u8(self.index), buf)?;
        // Autoincrement address, always zero when sending
        let buf = gen_simple(le_u16(0), buf)?;
        let buf = gen_simple(le_u16(self.register_address), buf)?;
        let buf = gen_simple(le_u16(u16::from_le_bytes(self.flags.pack().unwrap())), buf)?;
        let buf = gen_simple(le_u16(self.irq), buf)?;
        let buf = gen_simple(slice(self.data), buf)?;
        // Working counter is always zero when sending
        let buf = gen_simple(le_u16(0u16), buf)?;

        Ok(buf)
    }

    fn buf_len(&self) -> usize {
        // TODO: Add unit test to stop regressions
        N + 12
    }

    pub fn frame_buf_len(&self) -> usize {
        let size = self.buf_len() + mem::size_of::<FrameHeader>();

        // TODO: Move to unit test
        assert_eq!(size, N + 14);

        size
    }

    /// Write an EtherCAT PDU frame into the given buffer.
    pub fn as_ethercat_frame<'a>(&self, buf: &'a mut [u8]) -> Result<&'a mut [u8], GenError> {
        let header = FrameHeader::pdu(self.buf_len());

        let buf = gen_simple(le_u16(header.0), buf)?;
        let buf = self.as_bytes(buf)?;

        Ok(buf)
    }

    pub fn from_ethercat_frame<'a>(&self, i: &'a [u8]) -> IResult<&'a [u8], Option<Self>> {
        let (i, _header) = FrameHeader::parse_pdu(i)?;

        let (i, command_code) = map_res(nom::number::complete::u8, CommandCode::try_from)(i)?;

        let (i, index) = nom::number::complete::u8(i)?;

        // Possibly valid, but it's not our response
        if index != self.index {
            return Ok((i, None));
        }

        let (i, command) = command_code.parse_address(i)?;

        dbg!(&self.command, &command);

        // Check this is valid response to what we sent based on index
        if !self.command.is_valid_response(&command) {
            return Err(nom::Err::Error(nom::error::Error::new(
                i,
                ErrorKind::Verify,
            )));
        }

        let (i, register_address) = nom::number::complete::le_u16(i)?;
        let (i, flags) = map_res(take(2usize), PduFlags::unpack_from_slice)(i)?;
        let (i, irq) = nom::number::complete::le_u16(i)?;
        let (i, data) = map_res(take(N), |slice: &[u8]| slice.try_into())(i)?;
        let (i, working_counter) = nom::number::complete::le_u16(i)?;

        Ok((
            i,
            Some(Self {
                command,
                index,
                register_address,
                flags,
                irq,
                data,
                working_counter,
            }),
        ))
    }
}

#[derive(PartialEq, Debug)]
enum Command {
    Aprd {
        /// Auto increment counter.
        address: i16,
    },
    Fprd {
        /// Configured station address.
        address: u16,
    },
    Brd {
        /// Autoincremented by each slave visted.
        address: u16,
    },
    Lrd {
        /// Logical address.
        address: u32,
    },
}

impl Command {
    const fn code(&self) -> CommandCode {
        match self {
            Self::Aprd { .. } => CommandCode::Aprd,
            Self::Fprd { .. } => CommandCode::Fprd,
            Self::Brd { .. } => CommandCode::Brd,
            Self::Lrd { .. } => CommandCode::Lrd,
        }
    }

    fn as_bytes<'a>(&self, buf: &'a mut [u8]) -> Result<&'a mut [u8], GenError> {
        let buf = gen_simple(le_u8(self.code() as u8), buf)?;

        let buf = match self {
            Command::Aprd { address } => gen_simple(le_i16(*address), buf)?,
            Command::Fprd { address } | Command::Brd { address } => {
                gen_simple(le_u16(*address), buf)?
            }
            Command::Lrd { address } => gen_simple(le_u32(*address), buf)?,
        };

        Ok(buf)
    }

    fn is_valid_response(&self, other: &Self) -> bool {
        match self {
            // Ignore addresses for autoincrement services; the master sends zero and any slave
            // response is non-zero.
            Command::Aprd { .. } | Command::Brd { .. } => self.code() == other.code(),
            Command::Fprd { .. } => self == other,
            Command::Lrd { .. } => self == other,
        }
    }
}

/// Broadcast or configured station addressing.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum CommandCode {
    Aprd = 0x01,
    Fprd = 0x04,
    Brd = 0x07,
    Lrd = 0x0A,
}

impl CommandCode {
    /// Parse an address, producing a [`Command`].
    fn parse_address(self, i: &[u8]) -> IResult<&[u8], Command> {
        match self {
            Self::Aprd => map(nom::number::complete::le_i16, |address| Command::Aprd {
                address,
            })(i),
            Self::Fprd => map(nom::number::complete::le_u16, |address| Command::Fprd {
                address,
            })(i),
            Self::Brd => map(nom::number::complete::le_u16, |address| Command::Brd {
                address,
            })(i),
            Self::Lrd => map(nom::number::complete::le_u32, |address| Command::Lrd {
                address,
            })(i),
        }
    }
}

impl TryFrom<u8> for CommandCode {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0x01 => Ok(Self::Aprd),
            0x04 => Ok(Self::Fprd),
            0x07 => Ok(Self::Brd),
            0x0A => Ok(Self::Lrd),
            _ => Err(()),
        }
    }
}

#[derive(Copy, Clone, Debug, PackedStruct, PartialEq)]
// TODO: Fix endianness
#[packed_struct(size_bytes = "2", bit_numbering = "msb0", endian = "lsb")]
pub struct PduFlags {
    /// Data length of this PDU.
    #[packed_field(bits = "0..=10")]
    length: u16,
    #[packed_field(bits = "11..=13")]
    _reserved: u8,
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
            _reserved: 0,
            circulated: false,
            is_not_last: false,
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq)]
#[repr(transparent)]
struct FrameHeader(u16);

impl FrameHeader {
    fn pdu(len: usize) -> Self {
        // debug_assert!(len <= LEN_MASK.into());

        let len = (len as u16) & LEN_MASK;

        // TOOD: Const for PDU
        let protocol_type = 0x01 << 12;

        Self(len | protocol_type)
    }

    fn parse_pdu(i: &[u8]) -> IResult<&[u8], Self> {
        let (i, raw) = nom::number::complete::le_u16(i)?;

        let len = raw & LEN_MASK;

        // TODO: Take LEN_MASK + reserved bit and see if length has overflowed?

        let self_ = Self(raw);

        // TODO: Const
        if self_.protocol_type() == 0x01 {
            Ok((i, self_))
        } else {
            Err(nom::Err::Error(nom::error::Error::new(
                i,
                ErrorKind::Verify,
            )))
        }
    }

    fn len(&self) -> u16 {
        self.0 & LEN_MASK
    }

    // TODO: Return an enum
    fn protocol_type(&self) -> u8 {
        (self.0 >> 12) as u8 & 0b1111
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pdu_header() {
        let header = FrameHeader::pdu(0x28);

        let packed = header.0;

        let expected = 0b0001_0000_0010_1000;

        assert_eq!(packed, expected, "{packed:016b} == {expected:016b}");
    }

    #[test]
    fn decode_pdu_len() {
        let raw = 0b0001_0000_0010_1000;

        let header = FrameHeader(raw);

        assert_eq!(header.len(), 0x28);
        assert_eq!(header.protocol_type(), 0x01);
    }
}
