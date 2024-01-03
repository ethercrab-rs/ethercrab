//! Raw EtherCAT commands, e.g. `LRW`, `BRD`, `APWR`, etc.

use core::ops::Deref;

use crate::{
    error::{Error, PduError},
    fmt,
    generate::le_u16,
    pdu_loop::{CheckWorkingCounter, PduResponse, RxFrameDataBuf},
    Client,
};
use ethercrab_wire::{EtherCatWire, EtherCatWireSized};
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
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
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
    /// Wrap this command with a [`Client`]` to make it sendable over the wire.
    pub fn wrap<'client>(self, client: &'client Client<'client>) -> WrappedWrite<'client> {
        WrappedWrite::new(client, self)
    }
}

// impl Writes {
//     // /// Send a slice of data, returning the raw response.
//     // pub async fn send_receive_slice<'client, 'data>(
//     //     self,
//     //     client: &'client Client<'client>,
//     //     value: &'data [u8],
//     // ) -> Result<PduResponse<RxFrameDataBuf<'client>>, Error> {
//     //     client.write_service(self, value).await
//     // }

//     /// Call a method at least once to populate the frame payload buffer, then send it.
//     pub async fn send_receive_with<'client, 'data>(
//         self,
//         client: &'client Client<'client>,
//         d: impl EtherCatWire<'_>,
//     ) -> Result<PduResponse<RxFrameDataBuf<'client>>, Error> {
//         client
//             .pdu_loop
//             .send_packable(
//                 self.into(),
//                 d,
//                 None,
//                 client.timeouts.pdu,
//                 client.config.retry_behaviour,
//             )
//             .await
//             .map(|response| response.into_data())
//     }

//     // /// Send a slice of data, ignoring any response.
//     // pub async fn send_slice<'client, 'data>(
//     //     self,
//     //     client: &'client Client<'client>,
//     //     value: &'data [u8],
//     // ) -> Result<PduResponse<()>, Error> {
//     //     // client
//     //     //     .write_service(self, value)
//     //     //     .await
//     //     //     .map(|(_, wkc)| ((), wkc))

//     //     client
//     //         .pdu_loop
//     //         .send_packable(
//     //             self.into(),
//     //             value,
//     //             None,
//     //             client.timeouts.pdu,
//     //             client.config.retry_behaviour,
//     //         )
//     //         .await
//     //         .map(|response| ((), response.into_data().1))
//     // }

//     /// Send a slice but override the length parameter
//     pub(crate) async fn send_receive_slice_len<'client, 'data>(
//         self,
//         client: &'client Client<'client>,
//         value: &'data [u8],
//         len: u16,
//     ) -> Result<PduResponse<RxFrameDataBuf<'client>>, Error> {
//         // client.write_service_len(self, value, len).await

//         client
//             .pdu_loop
//             .send_packable(
//                 self.into(),
//                 value,
//                 Some(len),
//                 client.timeouts.pdu,
//                 client.config.retry_behaviour,
//             )
//             .await
//             .map(|response| response.into_data())
//     }

//     /// Send a value, ignoring any response from the network.
//     pub async fn send<'client, 'data, T>(
//         self,
//         client: &'client Client<'client>,
//         value: T,
//     ) -> Result<PduResponse<()>, Error>
//     where
//         T: for<'a> EtherCatWire<'a>,
//     {
//         // client
//         //     .write_service(self, value.as_slice())
//         //     .await
//         //     .map(|(_, wkc)| ((), wkc))

//         client
//             .pdu_loop
//             .send_packable(
//                 self.into(),
//                 value,
//                 None,
//                 client.timeouts.pdu,
//                 client.config.retry_behaviour,
//             )
//             .await
//             .map(|response| ((), response.into_data().1))
//     }

//     /// Send a value, returning the response sent by the slave network.
//     pub async fn send_receive<'client, 'data, T>(
//         self,
//         client: &'client Client<'client>,
//         value: T,
//     ) -> Result<PduResponse<T>, Error>
//     where
//         T: for<'a> EtherCatWire<'a>,
//     {
//         // client
//         //     .write_service(self, value.as_slice())
//         //     .await
//         //     .and_then(|(data, working_counter)| {
//         //         let res = T::try_from_slice(&data).map_err(|e| {
//         //             fmt::error!(
//         //                 "PDU data decode: {:?}, T: {} data {:?}",
//         //                 e,
//         //                 type_name::<T>(),
//         //                 data
//         //             );

//         //             PduError::Decode
//         //         })?;

//         //         Ok((res, working_counter))
//         //     })

//         client
//             .pdu_loop
//             .send_packable(
//                 self.into(),
//                 value,
//                 None,
//                 client.timeouts.pdu,
//                 client.config.retry_behaviour,
//             )
//             .await
//             .and_then(|response| {
//                 let (data, wkc) = response.into_data();

//                 let data = T::unpack_from_slice(&data)?;

//                 Ok((data, wkc))
//             })
//     }

//     pub(crate) async fn send_receive_slice_mut<'client, 'buf>(
//         self,
//         client: &'client Client<'client>,
//         value: &'buf mut [u8],
//         read_back_len: usize,
//     ) -> Result<PduResponse<&'buf mut [u8]>, Error> {
//         assert!(value.len() <= client.max_frame_data(), "Chunked sends not yet supported. Buffer of length {} is too long to send in one {} frame", value.len(), client.max_frame_data());

//         // let (data, working_counter) = client.write_service(self, value).await?;

//         // if data.len() != value.len() {
//         //     fmt::error!(
//         //         "Data length {} does not match value length {}",
//         //         data.len(),
//         //         value.len()
//         //     );
//         //     return Err(Error::Pdu(PduError::Decode));
//         // }

//         // value[0..read_back_len].copy_from_slice(&data[0..read_back_len]);

//         // Ok((value, working_counter))

//         let res = client
//             .pdu_loop
//             .send_packable(
//                 self.into(),
//                 value.as_ref(),
//                 None,
//                 client.timeouts.pdu,
//                 client.config.retry_behaviour,
//             )
//             .await?;

//         let (data, wkc) = res.into_data();

//         if data.len() != value.len() {
//             fmt::error!(
//                 "Data length {} does not match value length {}",
//                 data.len(),
//                 value.len()
//             );
//             return Err(Error::Pdu(PduError::Decode));
//         }

//         value[0..read_back_len].copy_from_slice(&data[0..read_back_len]);

//         Ok((value, wkc))
//     }
// }

/// Read commands that send no data.
#[derive(PartialEq, Eq, Debug, Copy, Clone)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
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
    /// Wrap this command with a client to make it sendable over the wire.
    pub fn wrap<'client>(self, client: &'client Client<'client>) -> WrappedRead<'client> {
        WrappedRead::new(client, self)
    }
}

// impl Reads {
//     /// Receive data and decode into a `T`.
//     pub async fn receive<T>(self, client: &Client<'_>) -> Result<PduResponse<T>, Error>
//     where
//         T: for<'a> EtherCatWireSized<'a>,
//     {
//         // client
//         //     .read_service(self, T::LEN)
//         //     .await
//         //     .and_then(|(data, working_counter)| {
//         //         let res = T::try_from_slice(&data).map_err(|e| {
//         //             fmt::error!(
//         //                 "PDU data decode: {:?}, T: {} data {:?}",
//         //                 e,
//         //                 type_name::<T>(),
//         //                 data
//         //             );

//         //             PduError::Decode
//         //         })?;

//         //         Ok((res, working_counter))
//         //     })

//         client
//             .pdu_loop
//             .send_packable(
//                 Command::Read(self),
//                 (),
//                 Some(T::BYTES as u16),
//                 client.timeouts.pdu,
//                 client.config.retry_behaviour,
//             )
//             .await
//             .and_then(|res| {
//                 let (data, wkc) = res.into_data();

//                 let data = T::unpack_from_slice(&data)?;

//                 Ok((data, wkc))
//             })
//     }

//     /// Receive a given number of bytes and return it as a slice.
//     pub async fn receive_slice<'client>(
//         self,
//         client: &'client Client<'client>,
//         len: u16,
//     ) -> Result<PduResponse<RxFrameDataBuf<'_>>, Error> {
//         // client.read_service(self, len).await

//         client
//             .pdu_loop
//             .send_packable(
//                 Command::Read(self),
//                 (),
//                 Some(len),
//                 client.timeouts.pdu,
//                 client.config.retry_behaviour,
//             )
//             .await
//             .map(|res| res.into_data())
//     }
// }

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

// pub struct PduBuilder<'client, P> {
//     command: Command,
//     len_override: Option<u16>,
//     // payload: PduPayload<'p, P>,
//     payload: P,
//     client: &'client Client<'client>,
//     context: &'static str,
//     wkc: u16,
// }

// impl<'client> PduBuilder<'client, ()> {
//     pub fn new(command: Command, client: &'client Client<'client>, context: &'static str) -> Self {
//         Self {
//             command,
//             len_override: None,
//             payload: (),
//             client,
//             context,
//             wkc: 1,
//         }
//     }

//     /// Set the expected working counter from its default of 1 to the given value.
//     pub fn set_wkc(mut self, wkc: u16) -> Self {
//         self.wkc = wkc;

//         self
//     }

//     /// Set the frame's length parameter in the header.
//     ///
//     /// This may be longer than the frame's data buffer as it is just metadata.
//     pub fn set_len(mut self, len: u16) -> Self {
//         self.len_override = Some(len);

//         self
//     }

//     /// Send the frame, ignoring the response.
//     pub async fn read<NEWP>(self) -> Result<(), Error>
//     where
//         NEWP: for<'a> EtherCatWireSized<'a>,
//     {
//         self.client
//             .pdu_loop
//             .send_packable(
//                 self.command,
//                 self.payload,
//                 Some(NEWP::BYTES as u16),
//                 self.client.timeouts.pdu,
//                 self.client.config.retry_behaviour,
//             )
//             .await?
//             .wkc(self.wkc, self.context)?;

//         Ok(())
//     }
// }

// impl<'client, P> PduBuilder<'client, P> {
//     /// Set the payload for this frame, along with its length.
//     pub fn set_payload<NEWP>(self, payload: NEWP) -> PduBuilder<'client, NEWP>
//     where
//         NEWP: for<'a> EtherCatWire<'a>,
//     {
//         PduBuilder {
//             payload,
//             command: self.command,
//             len_override: self.len_override,
//             context: self.context,
//             client: self.client,
//             wkc: self.wkc,
//         }
//     }
// }

// impl<'client, P> PduBuilder<'client, P>
// where
//     P: for<'a> EtherCatWire<'a>,
// {
//     /// Send the frame, ignoring the response.
//     pub async fn send(self) -> Result<(), Error> {
//         self.client
//             .pdu_loop
//             .send_packable(
//                 self.command,
//                 self.payload,
//                 None,
//                 self.client.timeouts.pdu,
//                 self.client.config.retry_behaviour,
//             )
//             .await?
//             .wkc(self.wkc, self.context)?;

//         Ok(())
//     }

//     /// Send the frame, waiting for and parsing the response.
//     pub async fn response<NEWP>(self) -> Result<NEWP, Error>
//     where
//         NEWP: for<'a> EtherCatWire<'a>,
//     {
//         let res = self
//             .client
//             .pdu_loop
//             .send_packable(
//                 self.command,
//                 self.payload,
//                 None,
//                 self.client.timeouts.pdu,
//                 self.client.config.retry_behaviour,
//             )
//             .await?
//             .wkc(self.wkc, self.context)
//             .and_then(|res| {
//                 let data = NEWP::unpack_from_slice(&res)?;

//                 Ok(data)
//             })?;

//         Ok(res)
//     }

//     /// Send the frame, waiting for and parsing the response.
//     pub async fn response_slice(self, len: usize) -> Result<RxFrameDataBuf<'client>, Error> {
//         self.client
//             .pdu_loop
//             .send_packable(
//                 self.command,
//                 (),
//                 Some(len as u16),
//                 self.client.timeouts.pdu,
//                 self.client.config.retry_behaviour,
//             )
//             .await?
//             .wkc(self.wkc, self.context)
//     }
// }

/// A wrapped version of a [`Reads`] exposing a builder API used to send/receive data over the wire.
pub struct WrappedRead<'client> {
    client: &'client Client<'client>,
    command: Reads,
    /// Expected working counter.
    wkc: Option<u16>,
}

impl<'client> WrappedRead<'client> {
    pub(crate) fn new(client: &'client Client<'client>, command: Reads) -> Self {
        Self {
            client,
            command,
            wkc: Some(1),
        }
    }

    /// Do not return an error if the working counter is different from the expected value.
    ///
    /// The default value is `1` and can be overridden with [`with_wkc`](WrappedRead::with_wkc).
    pub fn ignore_wkc(self) -> Self {
        Self { wkc: None, ..self }
    }

    /// Change the expected working counter from its default of `1`.
    pub fn with_wkc(self, wkc: u16) -> Self {
        Self {
            wkc: Some(wkc),
            ..self
        }
    }

    /// Receive data and decode into a `T`.
    pub async fn receive<T>(self) -> Result<T, Error>
    where
        T: for<'a> EtherCatWireSized<'a>,
    {
        self.client
            .pdu(self.command.into(), (), Some(T::BYTES as u16))
            .await
            .and_then(|(data, wkc)| {
                let data = T::unpack_from_slice(&data)?;

                Ok((data, wkc))
            })?
            .maybe_wkc(self.wkc)
    }

    /// Receive a given number of bytes and return it as a slice.
    pub async fn receive_slice(self, len: u16) -> Result<RxFrameDataBuf<'client>, Error> {
        self.client
            .pdu(self.command.into(), (), Some(len))
            .await?
            .maybe_wkc(self.wkc)
    }

    /// Receive only the working counter.
    ///
    /// `T` determines the length of the read, which is required for valid reads. It is otherwise
    /// ignored.
    pub(crate) async fn receive_wkc<T>(&self) -> Result<u16, Error>
    where
        T: for<'a> EtherCatWire<'a> + Default,
    {
        self.client
            .pdu(self.command.into(), T::default(), None)
            .await
            .map(|(_data, wkc)| wkc)
    }
}

/// A wrapped version of a [`Writes`] exposing a builder API used to send/receive data over the
/// wire.
pub struct WrappedWrite<'client> {
    client: &'client Client<'client>,
    command: Writes,
    /// Expected working counter.
    wkc: Option<u16>,
    len_override: Option<u16>,
}

impl<'client> WrappedWrite<'client> {
    pub(crate) fn new(client: &'client Client<'client>, command: Writes) -> Self {
        Self {
            client,
            command,
            wkc: Some(1),
            len_override: None,
        }
    }

    /// Set an explicit length for the PDU instead of taking it from the sent data.
    ///
    /// The length will be the _maximum_ of the value set here and the data sent.
    pub fn with_len(self, new_len: impl Into<u16>) -> Self {
        Self {
            len_override: Some(new_len.into()),
            ..self
        }
    }

    /// Do not return an error if the working counter is different from the expected value.
    ///
    /// The default value is `1` and can be overridden with [`with_wkc`](WrappedRead::with_wkc).
    pub fn ignore_wkc(self) -> Self {
        Self { wkc: None, ..self }
    }

    /// Change the expected working counter from its default of `1`.
    pub fn with_wkc(self, wkc: u16) -> Self {
        Self {
            wkc: Some(wkc),
            ..self
        }
    }

    /// Send a payload with a set length, ignoring the response.
    pub async fn send<'data>(self, data: impl EtherCatWire<'_>) -> Result<(), Error> {
        self.client
            .pdu(self.command.into(), data, self.len_override)
            .await?
            .maybe_wkc(self.wkc)?;

        Ok(())
    }

    /// Send a value, returning the response returned from the network.
    pub async fn send_receive<'data, T>(self, value: impl EtherCatWire<'_>) -> Result<T, Error>
    where
        T: for<'a> EtherCatWire<'a>,
    {
        self.client
            .pdu(self.command.into(), value, None)
            .await
            .and_then(|(data, wkc)| {
                let data = T::unpack_from_slice(&data)?;

                Ok((data, wkc))
            })?
            .maybe_wkc(self.wkc)
    }

    /// Similar to [`send_receive`](WrappedWrite::send_receive) but returns a slice.
    pub async fn send_receive_slice<'data>(
        self,
        value: impl EtherCatWire<'_>,
    ) -> Result<RxFrameDataBuf<'data>, Error>
    where
        'client: 'data,
    {
        self.client
            .pdu(self.command.into(), value, None)
            .await?
            .maybe_wkc(self.wkc)
    }

    pub(crate) async fn send_receive_slice_mut<'buf>(
        self,
        value: &'buf mut [u8],
        read_back_len: usize,
    ) -> Result<PduResponse<&[u8]>, Error> {
        assert!(
            value.len() <= self.client.max_frame_data(),
            "Chunked sends not yet supported. Buffer len {} B too long to send in {} B frame",
            value.len(),
            self.client.max_frame_data()
        );

        let (data, wkc) = self
            .client
            .pdu(self.command.into(), value.as_ref(), None)
            .await?;

        if data.len() != value.len() {
            fmt::error!(
                "Data length {} does not match value length {}",
                data.len(),
                value.len()
            );
            return Err(Error::Pdu(PduError::Decode));
        }

        value[0..read_back_len].copy_from_slice(&data[0..read_back_len]);

        Ok((&*value, wkc))
    }
}
