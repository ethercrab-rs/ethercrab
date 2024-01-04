use crate::{
    error::Error,
    pdu_loop::{CheckWorkingCounter, PduResponse, RxFrameDataBuf},
    Client,
};
use ethercrab_wire::EtherCrabWireSized;

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

    // Some manual monomorphisation
    async fn common(&self, len: u16) -> Result<PduResponse<RxFrameDataBuf<'client>>, Error> {
        self.client
            .pdu_loop
            .pdu_send(
                self.command.into(),
                (),
                Some(len),
                self.client.timeouts.pdu,
                self.client.config.retry_behaviour,
            )
            .await
            .map(|res| res.into_data())
    }

    /// Receive data and decode into a `T`.
    pub async fn receive<T>(self) -> Result<T, Error>
    where
        T: EtherCrabWireSized,
    {
        self.common(T::PACKED_LEN as u16)
            .await
            .and_then(|(data, wkc)| {
                let data = T::unpack_from_slice(&data)?;

                Ok((data, wkc))
            })?
            .maybe_wkc(self.wkc)
    }

    /// Receive a given number of bytes and return it as a slice.
    pub async fn receive_slice(self, len: u16) -> Result<RxFrameDataBuf<'client>, Error> {
        self.common(len).await?.maybe_wkc(self.wkc)
    }

    /// Receive only the working counter.
    ///
    /// Any expected working counter value will be ignored when calling this method, regardless of
    /// any value set by [`with_wkc`](WrappedRead::with_wkc).
    ///
    /// `T` determines the length of the read, which is required for valid reads. It is otherwise
    /// ignored.
    pub(crate) async fn receive_wkc<T>(&self) -> Result<u16, Error>
    where
        T: EtherCrabWireSized,
    {
        self.common(T::PACKED_LEN as u16)
            .await
            .map(|(_data, wkc)| wkc)
    }
}
