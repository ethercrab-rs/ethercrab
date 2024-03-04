use crate::{
    error::{Error, PduError},
    fmt,
    pdu_loop::{CheckWorkingCounter, PduResponse, RxFrameDataBuf},
    timer_factory::IntoTimeout,
    Client,
};
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

impl Writes {
    /// Wrap this command with a [`Client`]` to make it sendable over the wire.
    pub fn wrap<'client>(self, client: &'client Client<'client>) -> WrappedWrite<'client> {
        WrappedWrite::new(client, self)
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
    pub async fn send<'data>(self, data: impl EtherCrabWireWrite) -> Result<(), Error> {
        self.common(data, self.len_override).await?;

        Ok(())
    }

    /// Send a value, returning the response returned from the network.
    pub async fn send_receive<'data, T>(self, value: impl EtherCrabWireWrite) -> Result<T, Error>
    where
        T: EtherCrabWireRead,
    {
        self.common(value, None)
            .await?
            .maybe_wkc(self.wkc)
            .and_then(|data| Ok(T::unpack_from_slice(&data)?))
    }

    /// Similar to [`send_receive`](WrappedWrite::send_receive) but returns a slice.
    pub async fn send_receive_slice<'data>(
        self,
        value: impl EtherCrabWireWrite,
    ) -> Result<RxFrameDataBuf<'data>, Error>
    where
        'client: 'data,
    {
        self.common(value, None).await?.maybe_wkc(self.wkc)
    }

    // Some manual monomorphisation
    async fn common(
        &self,
        value: impl EtherCrabWireWrite,
        len_override: Option<u16>,
    ) -> Result<PduResponse<RxFrameDataBuf<'client>>, Error> {
        for _ in 0..self.client.config.retry_behaviour.loop_counts() {
            let (frame, frame_idx) =
                self.client
                    .pdu_loop
                    .pdu_send(self.command.into(), &value, len_override)?;

            match frame.timeout(self.client.timeouts.pdu).await {
                Ok(result) => return Ok(result.into_data()),
                Err(Error::Timeout) => {
                    fmt::error!("Frame {:#04x} timed out", frame_idx);

                    // NOTE: The `Drop` impl of `ReceiveFrameFut` frees the frame by setting its
                    // state to `None`, ready for reuse.
                }
                Err(e) => return Err(e),
            }
        }

        Err(Error::Timeout)
    }

    /// Send a slice, reading `read_back_len` bytes into the beginning of the provided slice.
    ///
    /// This is pretty much only useful for group TX/RX which returns bytes like `IIIIOOOO`, where
    /// `I` is where the sub devices write their input data to.
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

        let (data, wkc) = self.common(value.as_ref(), self.len_override).await?;

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
