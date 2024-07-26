use crate::{error::Error, pdu_loop::ReceivedPdu, MainDevice};
use ethercrab_wire::{EtherCrabWireRead, EtherCrabWireSized};

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
        /// Autoincremented by each SubDevice visited.
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
    /// Wrap this command with a MainDevice to make it sendable over the wire.
    pub fn wrap(self) -> WrappedRead {
        WrappedRead::new(self)
    }
}

/// A wrapped version of a [`Reads`] exposing a builder API used to send/receive data over the wire.
#[derive(Debug, Copy, Clone)]
pub struct WrappedRead {
    /// EtherCAT command.
    pub command: Reads,
    /// Expected working counter.
    wkc: Option<u16>,
}

impl WrappedRead {
    pub(crate) fn new(command: Reads) -> Self {
        Self {
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
    pub async fn receive<'maindevice, T>(
        self,
        maindevice: &'maindevice MainDevice<'maindevice>,
    ) -> Result<T, Error>
    where
        T: EtherCrabWireRead + EtherCrabWireSized,
    {
        self.common(maindevice, T::PACKED_LEN as u16)
            .await?
            .maybe_wkc(self.wkc)
            .and_then(|data| Ok(T::unpack_from_slice(&data)?))
    }

    /// Receive a given number of bytes and return it as a slice.
    pub async fn receive_slice<'maindevice>(
        self,
        maindevice: &'maindevice MainDevice<'maindevice>,
        len: u16,
    ) -> Result<ReceivedPdu<'maindevice>, Error> {
        self.common(maindevice, len).await?.maybe_wkc(self.wkc)
    }

    /// Receive only the working counter.
    ///
    /// Any expected working counter value will be ignored when calling this method, regardless of
    /// any value set by [`with_wkc`](WrappedRead::with_wkc).
    ///
    /// `T` determines the length of the read, which is required for valid reads. It is otherwise
    /// ignored.
    pub(crate) async fn receive_wkc<'maindevice, T>(
        &self,
        maindevice: &'maindevice MainDevice<'maindevice>,
    ) -> Result<u16, Error>
    where
        T: EtherCrabWireRead + EtherCrabWireSized,
    {
        self.common(maindevice, T::PACKED_LEN as u16)
            .await
            .map(|res| res.working_counter)
    }

    // Some manual monomorphisation
    fn common<'maindevice>(
        &self,
        maindevice: &'maindevice MainDevice<'maindevice>,
        len: u16,
    ) -> impl core::future::Future<Output = Result<ReceivedPdu<'maindevice>, Error>> {
        maindevice.single_pdu(self.command.into(), (), Some(len))
    }
}
