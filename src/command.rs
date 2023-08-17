use crate::{fmt, generate::le_u16};
use nom::{combinator::map, sequence::pair, IResult};

const NOP: u8 = 0x00;
const APRD: u8 = 0x01;
const FPRD: u8 = 0x04;
const BRD: u8 = 0x07;
const LRD: u8 = 0x0A;
const BWR: u8 = 0x08;
const APWR: u8 = 0x02;
const FPWR: u8 = 0x05;
const FRMW: u8 = 0x0E;
const LWR: u8 = 0x0B;
const LRW: u8 = 0x0c;

/// Write commands.
#[derive(PartialEq, Eq, Debug, Copy, Clone)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Writes {
    /// BWR.
    Bwr {
        /// Autoincremented by each slave visited.
        address: u16,

        /// Memory location to write to.
        register: u16,
    },
    /// APWR.
    Apwr {
        /// Auto increment counter.
        address: u16,

        /// Memory location to write to.
        register: u16,
    },
    /// FPWR.
    Fpwr {
        /// Configured station address.
        address: u16,

        /// Memory location to read from.
        register: u16,
    },
    /// FRMW.
    Frmw {
        /// Configured station address.
        address: u16,

        /// Memory location to read from.
        register: u16,
    },
    /// LWR.
    Lwr {
        /// Logical address.
        address: u32,
    },

    /// LRW.
    Lrw {
        /// Logical address.
        address: u32,
    },
}

/// Read commands that send no data.
#[derive(PartialEq, Eq, Debug, Copy, Clone)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Reads {
    /// APRD.
    Aprd {
        /// Auto increment counter.
        address: u16,

        /// Memory location to read from.
        register: u16,
    },
    /// FPRD.
    Fprd {
        /// Configured station address.
        address: u16,

        /// Memory location to read from.
        register: u16,
    },
    /// BRD.
    Brd {
        /// Autoincremented by each slave visited.
        address: u16,

        /// Memory location to read from.
        register: u16,
    },
    /// LRD.
    Lrd {
        /// Logical address.
        address: u32,
    },
}

/// PDU command.
#[derive(Default, PartialEq, Eq, Debug, Copy, Clone)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Command {
    /// No operation.
    #[default]
    Nop,

    /// Read commands.
    Read(Reads),

    /// Write commands.
    Write(Writes),
}

impl core::fmt::Display for Command {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Command::Nop => write!(f, "NOP"),

            Command::Read(read) => match read {
                Reads::Aprd { address, register } => {
                    write!(f, "APRD(addr {}, reg {})", address, register)
                }
                Reads::Fprd { address, register } => {
                    write!(f, "FPRD(addr {}, reg {}", address, register)
                }
                Reads::Brd { address, register } => {
                    write!(f, "BRD(addr {}, reg {}", address, register)
                }
                Reads::Lrd { address } => write!(f, "LRD(addr {})", address),
            },

            Command::Write(write) => match write {
                Writes::Bwr { address, register } => {
                    write!(f, "BWR(addr {}, reg {}", address, register)
                }
                Writes::Apwr { address, register } => {
                    write!(f, "APWR(addr {}, reg {}", address, register)
                }
                Writes::Fpwr { address, register } => {
                    write!(f, "FPWR(addr {}, reg {}", address, register)
                }
                Writes::Frmw { address, register } => {
                    write!(f, "FRMW(addr {}, reg {}", address, register)
                }
                Writes::Lwr { address } => write!(f, "LWR(addr {})", address),
                Writes::Lrw { address } => write!(f, "LRW(addr {})", address),
            },
        }
    }
}

impl Command {
    /// Get just the command code for a command.
    pub const fn code(&self) -> u8 {
        match self {
            Self::Nop => NOP,

            Self::Read(read) => match read {
                Reads::Aprd { .. } => APRD,
                Reads::Fprd { .. } => FPRD,
                Reads::Brd { .. } => BRD,
                Reads::Lrd { .. } => LRD,
            },

            Self::Write(write) => match write {
                Writes::Bwr { .. } => BWR,
                Writes::Apwr { .. } => APWR,
                Writes::Fpwr { .. } => FPWR,
                Writes::Frmw { .. } => FRMW,
                Writes::Lwr { .. } => LWR,
                Writes::Lrw { .. } => LRW,
            },
        }
    }

    /// Get the address value for the command.
    pub fn address(&self) -> [u8; 4] {
        let mut arr = [0x00u8; 4];

        let buf = arr.as_mut_slice();

        match *self {
            Command::Nop => arr,

            Command::Read(Reads::Aprd { address, register })
            | Command::Read(Reads::Brd { address, register })
            | Command::Read(Reads::Fprd { address, register })
            | Command::Write(Writes::Apwr { address, register })
            | Command::Write(Writes::Fpwr { address, register })
            | Command::Write(Writes::Frmw { address, register })
            | Command::Write(Writes::Bwr { address, register }) => {
                let buf = le_u16(address, buf);
                let _buf = le_u16(register, buf);

                arr
            }
            Command::Read(Reads::Lrd { address })
            | Command::Write(Writes::Lwr { address })
            | Command::Write(Writes::Lrw { address }) => address.to_le_bytes(),
        }
    }

    /// Parse a command from a code and address data (e.g. `(u16, u16)` or `u32`), producing a [`Command`].
    pub fn parse(command_code: u8, i: &[u8]) -> IResult<&[u8], Self> {
        use nom::number::complete::{le_u16, le_u32};

        match command_code {
            NOP => Ok((i, Command::Nop)),

            APRD => map(pair(le_u16, le_u16), |(address, register)| {
                Command::Read(Reads::Aprd { address, register })
            })(i),
            FPRD => map(pair(le_u16, le_u16), |(address, register)| {
                Command::Read(Reads::Fprd { address, register })
            })(i),
            BRD => map(pair(le_u16, le_u16), |(address, register)| {
                Command::Read(Reads::Brd { address, register })
            })(i),
            LRD => map(le_u32, |address| Command::Read(Reads::Lrd { address }))(i),

            BWR => map(pair(le_u16, le_u16), |(address, register)| {
                Command::Write(Writes::Bwr { address, register })
            })(i),
            APWR => map(pair(le_u16, le_u16), |(address, register)| {
                Command::Write(Writes::Apwr { address, register })
            })(i),
            FPWR => map(pair(le_u16, le_u16), |(address, register)| {
                Command::Write(Writes::Fpwr { address, register })
            })(i),
            FRMW => map(pair(le_u16, le_u16), |(address, register)| {
                Command::Write(Writes::Frmw { address, register })
            })(i),
            LWR => map(le_u32, |address| Command::Write(Writes::Lwr { address }))(i),

            LRW => map(le_u32, |address| Command::Write(Writes::Lrw { address }))(i),

            other => {
                fmt::error!("Invalid command code {:#02x}", other);

                Err(nom::Err::Failure(nom::error::Error {
                    input: i,
                    code: nom::error::ErrorKind::Tag,
                }))
            }
        }
    }
}
