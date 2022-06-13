use super::LEN_MASK;
use nom::{
    bytes::complete::{tag, take},
    combinator::{map_res, verify},
    error::context,
    number::complete::{self, le_u16},
    IResult,
};
use std::io::{self, Write};

#[derive(Debug)]
pub enum Pdu {
    Fprd(Fprd),
    Brd(Brd),
}

impl Pdu {
    pub fn byte_len(&self) -> u16 {
        match self {
            Self::Fprd(c) => c.byte_len(),
            Self::Brd(c) => c.byte_len(),
        }
    }

    pub fn as_bytes(&self, buf: &mut [u8]) -> io::Result<()> {
        match self {
            Self::Fprd(c) => c.as_bytes(buf),
            Self::Brd(c) => c.as_bytes(buf),
        }
    }

    pub(crate) fn set_has_next(&mut self, has_next: bool) {
        match self {
            Self::Fprd(c) => c.set_has_next(has_next),
            Self::Brd(c) => c.set_has_next(has_next),
        }
    }
}

#[derive(Debug)]
pub struct Fprd {
    // TODO: Make this the `Command` enum
    command: u8,
    idx: u8,
    adp: u16,
    /// Memory or register address.
    ado: u16,
    /// len(11), reserved(3), circulating(1), next(1)
    packed: u16,
    irq: u16,
    /// Read buffer containing response from slave.
    data: Vec<u8>,
    working_counter: u16,

    // Not in PDU
    data_len: u16,
}

impl Fprd {
    pub fn new(len: u16, slave_addr: u16, memory_address: u16) -> Self {
        // Other fields are all zero for now
        let packed = len & LEN_MASK;

        Self {
            command: Command::Fprd as u8,
            idx: 0x01,
            adp: slave_addr,
            ado: memory_address,
            packed,
            irq: 0,
            data: Vec::with_capacity(len.into()),
            working_counter: 0,
            data_len: len,
        }
    }

    /// Length of this entire struct in bytes
    pub fn byte_len(&self) -> u16 {
        let static_len = 12;

        static_len + u16::try_from(self.data_len).expect("Too long")
    }

    pub fn as_bytes(&self, mut buf: &mut [u8]) -> io::Result<()> {
        buf.write_all(&[self.command])?;
        buf.write_all(&[self.idx])?;
        buf.write_all(&self.adp.to_le_bytes())?;
        buf.write_all(&self.ado.to_le_bytes())?;
        buf.write_all(&self.packed.to_le_bytes())?;
        buf.write_all(&self.irq.to_le_bytes())?;
        // Populate data payload with zeroes. The slave will write data into this section.
        buf.write_all(&[0x00].repeat(self.data_len.into()))?;
        buf.write_all(&self.working_counter.to_le_bytes())?;

        Ok(())
    }

    fn set_has_next(&mut self, has_next: bool) {
        let flag = u16::from(has_next) << 15;

        self.packed |= flag;
    }
}

#[derive(Debug, Clone)]
pub struct Brd {
    // TODO: Make this the `Command` enum
    command: u8,
    idx: u8,
    adp: u16,
    /// Memory or register address.
    ado: u16,
    /// len(11), reserved(3), circulating(1), next(1)
    packed: u16,
    irq: u16,
    /// Read buffer containing response from slave.
    data: Vec<u8>,
    working_counter: u16,

    // Not in PDU
    data_len: u16,
}

impl Brd {
    pub fn new(len: u16, memory_address: u16) -> Self {
        // Other fields are all zero for now
        let packed = len & LEN_MASK;

        Self {
            command: Command::Brd as u8,
            idx: 0x01,
            adp: 0,
            ado: memory_address,
            packed,
            irq: 0,
            data: Vec::with_capacity(len.into()),
            working_counter: 0,
            data_len: len,
        }
    }

    /// Length of this entire struct in bytes
    pub fn byte_len(&self) -> u16 {
        let static_len = 12;

        static_len + u16::try_from(self.data_len).expect("Too long")
    }

    pub fn as_bytes(&self, mut buf: &mut [u8]) -> io::Result<()> {
        buf.write_all(&[self.command])?;
        buf.write_all(&[self.idx])?;
        buf.write_all(&self.adp.to_le_bytes())?;
        buf.write_all(&self.ado.to_le_bytes())?;
        buf.write_all(&self.packed.to_le_bytes())?;
        buf.write_all(&self.irq.to_le_bytes())?;
        // Populate data payload with zeroes. The slave will write data into this section.
        buf.write_all(&[0x00].repeat(self.data_len.into()))?;
        buf.write_all(&self.working_counter.to_le_bytes())?;

        Ok(())
    }

    fn set_has_next(&mut self, has_next: bool) {
        let flag = u16::from(has_next) << 15;

        self.packed |= flag;
    }

    pub fn parse_response<'a, 'b>(&'a self, i: &'b [u8]) -> Result<Self, PduParseError> {
        let (i, command) = context(
            "command",
            map_res(complete::u8, |b| {
                if b == (Command::Brd as u8) {
                    Ok(b)
                } else {
                    dbg!(b, Command::Brd as u8);
                    Err(PduParseError::Command {
                        received: b
                            .try_into()
                            .map_err(|unknown| PduParseError::InvalidCommand(unknown))?,
                        expected: self
                            .command
                            .try_into()
                            .map_err(|unknown| PduParseError::InvalidCommand(unknown))?,
                    })
                }
            }),
        )(i)?;
        let (i, idx) = map_res(complete::u8, |idx| {
            if idx == self.idx {
                Ok(idx)
            } else {
                Err(PduParseError::Index {
                    received: idx,
                    expected: self.idx,
                })
            }
        })(i)?;
        let (i, adp) = le_u16(i)?;
        let (i, ado) = le_u16(i)?;
        let (i, packed) = le_u16(i)?;
        let (i, irq) = le_u16(i)?;

        let data_len = packed & LEN_MASK;

        let (i, data) = take(data_len)(i)?;
        let (i, working_counter) = le_u16(i)?;

        if !i.is_empty() {
            return Err(PduParseError::Incomplete);
        }

        Ok(Self {
            command,
            idx,
            adp,
            ado,
            packed,
            irq,
            data: data.to_vec(),
            working_counter,
            data_len,
        })
    }

    /// Check if the given packet is in response to self.
    pub fn is_response(&self, other: &Self) -> bool {
        self.command == other.command && self.idx == other.idx
    }

    pub fn wkc(&self) -> u16 {
        self.working_counter
    }
}

#[derive(Debug)]
// TODO: thiserror
pub enum PduParseError {
    Command {
        received: Command,
        expected: Command,
    },
    Index {
        received: u8,
        expected: u8,
    },
    Parse(String),
    Incomplete,
    InvalidCommand(u8),
}

impl From<nom::Err<nom::error::VerboseError<&[u8]>>> for PduParseError {
    fn from(e: nom::Err<nom::error::VerboseError<&[u8]>>) -> Self {
        PduParseError::Parse(e.to_string())
    }
}

#[derive(Debug)]
pub enum Command {
    Fprd = 0x04,
    Brd = 0x07,
}

impl TryFrom<u8> for Command {
    type Error = u8;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0x04 => Ok(Self::Fprd),
            0x07 => Ok(Self::Brd),
            unknown => Err(unknown),
        }
    }
}
