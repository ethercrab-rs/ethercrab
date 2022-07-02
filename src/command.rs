use cookie_factory::{gen_simple, GenError};
use nom::{combinator::map, sequence::pair, IResult};

#[derive(PartialEq, Debug)]
pub enum Command {
    Aprd {
        /// Auto increment counter.
        address: i16,

        /// Memory location to read from.
        register: u16,
    },
    Fprd {
        /// Configured station address.
        address: u16,

        /// Memory location to read from.
        register: u16,
    },
    Brd {
        /// Autoincremented by each slave visted.
        address: u16,

        /// Memory location to read from.
        register: u16,
    },
    Lrd {
        /// Logical address.
        address: u32,
    },

    Fpwr {
        /// Configured station address.
        address: u16,

        /// Memory location to read from.
        register: u16,
    },
}

impl Command {
    pub const fn code(&self) -> CommandCode {
        match self {
            // Reads
            Self::Aprd { .. } => CommandCode::Aprd,
            Self::Fprd { .. } => CommandCode::Fprd,
            Self::Brd { .. } => CommandCode::Brd,
            Self::Lrd { .. } => CommandCode::Lrd,

            // Writes
            Self::Fpwr { .. } => CommandCode::Fpwr,
        }
    }

    pub fn address(&self) -> Result<[u8; 4], GenError> {
        let mut arr = [0x00u8; 4];

        let buf = arr.as_mut_slice();

        match *self {
            Command::Aprd { address, register } => {
                let buf = gen_simple(cookie_factory::bytes::le_i16(address), buf)?;
                gen_simple(cookie_factory::bytes::le_u16(register), buf)
            }
            Command::Fprd { address, register }
            | Command::Fpwr { address, register }
            | Command::Brd { address, register } => {
                let buf = gen_simple(cookie_factory::bytes::le_u16(address), buf)?;
                gen_simple(cookie_factory::bytes::le_u16(register), buf)
            }
            Command::Lrd { address } => gen_simple(cookie_factory::bytes::le_u32(address), buf),
        }?;

        Ok(arr)
    }

    /// Compare another command and address against self.
    ///
    /// Commands which cause address autoincrements during slave traversal will not compare
    /// addresses.
    pub fn is_valid_response(&self, other: &Self) -> bool {
        match self {
            // Ignore addresses for autoincrement services; the master sends zero and any slave
            // response is non-zero.
            Command::Aprd { .. } | Command::Brd { .. } => self.code() == other.code(),
            _ => self == other,
        }
    }
}

/// Broadcast or configured station addressing.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum CommandCode {
    // Reads
    Aprd = 0x01,
    Fprd = 0x04,
    Brd = 0x07,
    Lrd = 0x0A,

    // Writes
    Fpwr = 0x05,
}

impl CommandCode {
    /// Parse an address, producing a [`Command`].
    pub fn parse_address(self, i: &[u8]) -> IResult<&[u8], Command> {
        use nom::number::complete::{le_i16, le_u16, le_u32};

        match self {
            Self::Aprd => map(pair(le_i16, le_u16), |(address, register)| Command::Aprd {
                address,
                register,
            })(i),
            Self::Fprd => map(pair(le_u16, le_u16), |(address, register)| Command::Fprd {
                address,
                register,
            })(i),
            Self::Brd => map(pair(le_u16, le_u16), |(address, register)| Command::Brd {
                address,
                register,
            })(i),
            Self::Lrd => map(le_u32, |address| Command::Lrd { address })(i),

            Self::Fpwr => map(pair(le_u16, le_u16), |(address, register)| Command::Fpwr {
                address,
                register,
            })(i),
        }
    }
}

impl TryFrom<u8> for CommandCode {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            // Reads
            0x01 => Ok(Self::Aprd),
            0x04 => Ok(Self::Fprd),
            0x07 => Ok(Self::Brd),
            0x0A => Ok(Self::Lrd),

            // Writes
            0x05 => Ok(Self::Fpwr),
            _ => Err(()),
        }
    }
}
