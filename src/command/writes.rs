use super::common;
use crate::{error::Error, fmt, pdu_loop::ReceivedPdu, Client};
use core::future::Future;
use ethercrab_wire::{EtherCrabWireRead, EtherCrabWireWrite};

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

/// A wrapped version of a [`Writes`] exposing a builder API used to send/receive data over the
/// wire.
#[derive(Debug, Copy, Clone)]
pub struct WrappedWrite {
    /// EtherCAT command.
    pub command: Writes,

    /// Expected working counter.
    wkc: Option<u16>,
    len_override: Option<u16>,
}

impl WrappedWrite {
    pub(crate) fn new(command: Writes) -> Self {
        Self {
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
    /// The default value is `1` and can be overridden with [`with_wkc`](WrappedWrite::with_wkc).
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

    /// Send a payload with a length set by [`with_len`](WrappedWrite::with_len), ignoring the
    /// response.
    pub async fn send<'data, 'client>(
        self,
        client: &'client Client<'client>,
        data: impl EtherCrabWireWrite,
    ) -> Result<(), Error> {
        let _ = common(client, self.command.into(), data, self.len_override).await?;

        Ok(())
    }

    /// Send a value, returning the response returned from the network.
    pub fn send_receive<'data, 'client, T>(
        self,
        client: &'client Client<'client>,
        value: impl EtherCrabWireWrite + 'data,
    ) -> impl Future<Output = Result<T, Error>> + 'data
    where
        T: EtherCrabWireRead,
        'client: 'data,
    {
        async move {
            common(client, self.command.into(), value, None)
                .await?
                .maybe_wkc(self.wkc)
                .and_then(|data| T::unpack_from_slice(&data).map_err(Error::from))
        }
    }

    /// Similar to [`send_receive`](WrappedWrite::send_receive) but returns a slice.
    pub async fn send_receive_slice<'data, 'client>(
        self,
        client: &'client Client<'client>,
        value: impl EtherCrabWireWrite,
    ) -> Result<ReceivedPdu<'data, ()>, Error>
    where
        'client: 'data,
    {
        common(client, self.command.into(), value, None)
            .await?
            .maybe_wkc(self.wkc)
    }
}
