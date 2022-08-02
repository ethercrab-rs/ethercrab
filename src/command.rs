use cookie_factory::{gen_simple, GenError};
use nom::{combinator::map, error::ParseError, sequence::pair, IResult};

#[derive(PartialEq, Debug, Copy, Clone)]
pub enum Command {
    Aprd {
        /// Auto increment counter.
        address: u16,

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
        /// Autoincremented by each slave visited.
        address: u16,

        /// Memory location to read from.
        register: u16,
    },
    Lrd {
        /// Logical address.
        address: u32,
    },

    Bwr {
        /// Autoincremented by each slave visited.
        address: u16,

        /// Memory location to write to.
        register: u16,
    },
    Apwr {
        /// Auto increment counter.
        address: u16,

        /// Memory location to write to.
        register: u16,
    },
    Fpwr {
        /// Configured station address.
        address: u16,

        /// Memory location to read from.
        register: u16,
    },
    Lwr {
        /// Logical address.
        address: u32,
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
            Self::Bwr { .. } => CommandCode::Bwr,
            Self::Apwr { .. } => CommandCode::Apwr,
            Self::Fpwr { .. } => CommandCode::Fpwr,
            Self::Lwr { .. } => CommandCode::Lwr,
        }
    }

    pub fn address(&self) -> Result<[u8; 4], GenError> {
        let mut arr = [0x00u8; 4];

        let buf = arr.as_mut_slice();

        match *self {
            Command::Aprd { address, register }
            | Command::Apwr { address, register }
            | Command::Fprd { address, register }
            | Command::Fpwr { address, register }
            | Command::Brd { address, register }
            | Command::Bwr { address, register } => {
                let buf = gen_simple(cookie_factory::bytes::le_u16(address), buf)?;
                gen_simple(cookie_factory::bytes::le_u16(register), buf)
            }
            Command::Lrd { address } | Command::Lwr { address } => {
                gen_simple(cookie_factory::bytes::le_u32(address), buf)
            }
        }?;

        Ok(arr)
    }
}

/// Broadcast or configured station addressing.
#[derive(Copy, Clone, Debug, PartialEq, Eq, num_enum::TryFromPrimitive)]
#[repr(u8)]
pub enum CommandCode {
    // Reads
    Aprd = 0x01,
    Fprd = 0x04,
    Brd = 0x07,
    Lrd = 0x0A,

    // Writes
    Bwr = 0x08,
    Apwr = 0x02,
    Fpwr = 0x05,
    Lwr = 0x0B,
}

impl CommandCode {
    /// Parse an address, producing a [`Command`].
    pub fn parse_address<'a, E>(self, i: &'a [u8]) -> IResult<&'a [u8], Command, E>
    where
        E: ParseError<&'a [u8]>,
    {
        use nom::number::complete::{le_u16, le_u32};

        match self {
            Self::Aprd => map(pair(le_u16, le_u16), |(address, register)| Command::Aprd {
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

            Self::Bwr => map(pair(le_u16, le_u16), |(address, register)| Command::Bwr {
                address,
                register,
            })(i),
            Self::Apwr => map(pair(le_u16, le_u16), |(address, register)| Command::Apwr {
                address,
                register,
            })(i),
            Self::Fpwr => map(pair(le_u16, le_u16), |(address, register)| Command::Fpwr {
                address,
                register,
            })(i),
            Self::Lwr => map(le_u32, |address| Command::Lwr { address })(i),
        }
    }
}
