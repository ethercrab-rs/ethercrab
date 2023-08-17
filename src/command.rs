use core::any::type_name;

use crate::{
    error::{Error, PduError},
    fmt,
    generate::le_u16,
    pdu_data::{PduData, PduRead},
    pdu_loop::{PduResponse, RxFrameDataBuf},
    Client,
};
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

impl Writes {
    /// Send a slice of data, returning the raw response.
    pub async fn send_receive_slice<'client, 'data>(
        self,
        client: &'client Client<'client>,
        value: &'data [u8],
    ) -> Result<PduResponse<RxFrameDataBuf<'client>>, Error> {
        client.write_service_inner(self, value).await
    }

    /// Send a slice but override the length parameter
    pub async fn send_receive_slice_len<'client, 'data>(
        self,
        client: &'client Client<'client>,
        value: &'data [u8],
        len: u16,
    ) -> Result<PduResponse<RxFrameDataBuf<'client>>, Error> {
        client.write_service_len(self, value, len).await
    }

    /// Send a value.
    pub async fn send_receive<'client, 'data, T>(
        self,
        client: &'client Client<'client>,
        value: T,
    ) -> Result<PduResponse<T>, Error>
    where
        T: PduData,
    {
        client
            .write_service_inner(self, value.as_slice())
            .await
            .and_then(|(data, working_counter)| {
                let res = T::try_from_slice(&*data).map_err(|e| {
                    fmt::error!(
                        "PDU data decode: {:?}, T: {} data {:?}",
                        e,
                        type_name::<T>(),
                        data
                    );

                    PduError::Decode
                })?;

                Ok((res, working_counter))
            })
    }

    pub async fn send_receive_slice_mut<'client, 'buf>(
        self,
        client: &'client Client<'client>,
        value: &'buf mut [u8],
        read_back_len: usize,
    ) -> Result<PduResponse<&'buf mut [u8]>, Error> {
        assert!(value.len() <= client.max_frame_data(), "Chunked sends not yet supported. Buffer of length {} is too long to send in one {} frame", value.len(), client.max_frame_data());

        let (data, working_counter) = client.write_service_inner(self, value).await?;

        if data.len() != value.len() {
            fmt::error!(
                "Data length {} does not match value length {}",
                data.len(),
                value.len()
            );
            return Err(Error::Pdu(PduError::Decode));
        }

        value[0..read_back_len].copy_from_slice(&data[0..read_back_len]);

        Ok((value, working_counter))
    }
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
    /// Broadcast Read (BRD).
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
    /// FRMW.
    Frmw {
        /// Configured station address.
        address: u16,

        /// Memory location to read from.
        register: u16,
    },
}

impl Reads {
    /// Receive data and decode into a `T`.
    pub async fn receive<T>(self, client: &Client<'_>) -> Result<PduResponse<T>, Error>
    where
        T: PduRead,
    {
        client
            .read_service_inner(self, T::LEN)
            .await
            .and_then(|(data, working_counter)| {
                let res = T::try_from_slice(&*data).map_err(|e| {
                    fmt::error!(
                        "PDU data decode: {:?}, T: {} data {:?}",
                        e,
                        type_name::<T>(),
                        data
                    );

                    PduError::Decode
                })?;

                Ok((res, working_counter))
            })
    }

    /// Receive a given number of bytes and return it as a slice.
    pub async fn receive_slice<'client>(
        self,
        client: &'client Client<'client>,
        len: u16,
    ) -> Result<PduResponse<RxFrameDataBuf<'_>>, Error> {
        client.read_service_inner(self, len).await
    }
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
                Reads::Frmw { address, register } => {
                    write!(f, "FRMW(addr {}, reg {}", address, register)
                }
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
    pub const fn code(&self) -> u8 {
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
    pub fn address(&self) -> [u8; 4] {
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
