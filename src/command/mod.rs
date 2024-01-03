//! Raw EtherCAT commands, e.g. `LRW`, `BRD`, `APWR`, etc.
//!
mod reads;
mod writes;

use crate::{fmt, generate::le_u16};
use nom::{combinator::map, sequence::pair, IResult};

pub use reads::{Reads, WrappedRead};
pub use writes::{WrappedWrite, Writes};

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

/// PDU command.
///
/// A command can be used in various different ways, to e.g. read a number or write a raw slice to a
/// slave device on the network.
///
/// All EtherCAT commands are implemented. It is recommended to use the methods on `Command` to
/// create them.
///
/// A `Command` won't do much on its own. To perform network operations with the command it must be
/// wrapped with either [`WrappedRead`] or [`WrappedWrite`]. These structs add a [`Client`]` and
/// expose many different read/write operations. See the methods on [`WrappedRead`] and
/// [`WrappedWrite`] for more.
///
/// # Examples
///
/// ## Read a `u32` from a slave by address
///
/// ```rust
/// # use ethercrab::{ std::tx_rx_task, Client, ClientConfig, PduStorage, Timeouts };
/// use ethercrab::{ Command, RegisterAddress };
/// # static PDU_STORAGE: PduStorage<16, 1100> = PduStorage::new();
/// # let (_tx, _rx, pdu_loop) = PDU_STORAGE.try_split().expect("can only split once");
/// let client = /* ... */
/// # Client::new(pdu_loop, Timeouts::default(), ClientConfig::default());
///
/// let slave_configured_address = 0x1001u16;
///
/// # async {
/// let value = Command::fprd(slave_configured_address, RegisterAddress::SiiData.into())
///     .wrap(&client)
///     .receive::<u32>()
///     .await?;
/// # Result::<(), ethercrab::error::Error>::Ok(())
/// # };
/// ```
///
/// ## Write a slice to a given slave address and register
///
/// ```rust
/// # use ethercrab::{ std::tx_rx_task, Client, ClientConfig, PduStorage, Timeouts };
/// use ethercrab::{ Command, RegisterAddress };
/// # static PDU_STORAGE: PduStorage<16, 1100> = PduStorage::new();
/// # let (_tx, _rx, pdu_loop) = PDU_STORAGE.try_split().expect("can only split once");
/// let client = /* ... */
/// # Client::new(pdu_loop, Timeouts::default(), ClientConfig::default());
///
/// let slave_configured_address = 0x1001u16;
/// let register = 0x1234u16;
///
/// let data = [ 0xaau8, 0xbb, 0xcc, 0xdd ];
///
/// # async {
/// Command::fpwr(slave_configured_address, register)
///     .wrap(&client)
///     .send(data)
///     .await?;
/// # Result::<(), ethercrab::error::Error>::Ok(())
/// # };
/// ```
#[derive(Default, PartialEq, Eq, Debug, Copy, Clone)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
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
                    write!(f, "FPRD(addr {}, reg {})", address, register)
                }
                Reads::Brd { address, register } => {
                    write!(f, "BRD(addr {}, reg {})", address, register)
                }
                Reads::Lrd { address } => write!(f, "LRD(addr {})", address),
                Reads::Frmw { address, register } => {
                    write!(f, "FRMW(addr {}, reg {})", address, register)
                }
            },

            Command::Write(write) => match write {
                Writes::Bwr { address, register } => {
                    write!(f, "BWR(addr {}, reg {})", address, register)
                }
                Writes::Apwr { address, register } => {
                    write!(f, "APWR(addr {}, reg {})", address, register)
                }
                Writes::Fpwr { address, register } => {
                    write!(f, "FPWR(addr {}, reg {})", address, register)
                }

                Writes::Lwr { address } => write!(f, "LWR(addr {})", address),
                Writes::Lrw { address } => write!(f, "LRW(addr {})", address),
            },
        }
    }
}

impl Command {
    /// Create a broadcast read (BRD) command to the given register address.
    ///
    /// The configured station address is always zero when transmitted from the master.
    pub fn brd(register: u16) -> Reads {
        Reads::Brd {
            // This is a broadcast, so the address is always zero when sent from the master
            address: 0,
            register,
        }
    }

    /// Create a broadcast write (BWR) command to the given register address.
    ///
    /// The configured station address is always zero when transmitted from the master.
    pub fn bwr(register: u16) -> Writes {
        Writes::Bwr {
            // This is a broadcast, so the address is always zero when sent from the master
            address: 0,
            register,
        }
    }

    /// FPRD.
    pub fn fprd(address: u16, register: u16) -> Reads {
        Reads::Fprd { address, register }
    }

    /// FPWR.
    pub fn fpwr(address: u16, register: u16) -> Writes {
        Writes::Fpwr { address, register }
    }

    /// APRD.
    pub fn aprd(address: u16, register: u16) -> Reads {
        Reads::Aprd {
            address: 0u16.wrapping_sub(address),
            register,
        }
    }

    /// APWR.
    pub fn apwr(address: u16, register: u16) -> Writes {
        Writes::Apwr {
            address: 0u16.wrapping_sub(address),
            register,
        }
    }

    /// Configured address read, multiple write (FRMW).
    ///
    /// This can be used to distribute a value from one slave to all others on the network, e.g.
    /// with distributed clocks.
    pub fn frmw(address: u16, register: u16) -> Reads {
        Reads::Frmw { address, register }
    }

    /// Logical Read Write (LRD), used mainly for sending and receiving PDI.
    pub fn lrw(address: u32) -> Writes {
        Writes::Lrw { address }
    }

    /// Logical Write (LWR).
    pub fn lwr(address: u32) -> Writes {
        Writes::Lwr { address }
    }

    /// Get just the command code for a command.
    pub(crate) const fn code(&self) -> u8 {
        match self {
            Self::Nop => NOP,

            Self::Read(read) => match read {
                Reads::Aprd { .. } => APRD,
                Reads::Fprd { .. } => FPRD,
                Reads::Brd { .. } => BRD,
                Reads::Lrd { .. } => LRD,
                Reads::Frmw { .. } => FRMW,
            },

            Self::Write(write) => match write {
                Writes::Bwr { .. } => BWR,
                Writes::Apwr { .. } => APWR,
                Writes::Fpwr { .. } => FPWR,
                Writes::Lwr { .. } => LWR,
                Writes::Lrw { .. } => LRW,
            },
        }
    }

    /// Get the address value for the command.
    pub(crate) fn address(&self) -> [u8; 4] {
        let mut arr = [0x00u8; 4];

        let buf = arr.as_mut_slice();

        match *self {
            Command::Nop => arr,

            Command::Read(Reads::Aprd { address, register })
            | Command::Read(Reads::Brd { address, register })
            | Command::Read(Reads::Fprd { address, register })
            | Command::Read(Reads::Frmw { address, register })
            | Command::Write(Writes::Apwr { address, register })
            | Command::Write(Writes::Fpwr { address, register })
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
    pub(crate) fn parse(command_code: u8, i: &[u8]) -> IResult<&[u8], Self> {
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
                Command::Read(Reads::Frmw { address, register })
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

impl From<Reads> for Command {
    fn from(value: Reads) -> Self {
        Self::Read(value)
    }
}

impl From<Writes> for Command {
    fn from(value: Writes) -> Self {
        Self::Write(value)
    }
}
