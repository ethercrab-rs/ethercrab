use crate::{
    error::Error,
    pdu_loop::{CheckWorkingCounter, PduResponse, RxFrameDataBuf},
    Client, Command, Timeouts,
};
use ethercrab_wire::{EtherCrabWireReadSized, EtherCrabWireReadWrite, EtherCrabWireWrite};

/// A wrapper around [`Client`] preconfigured to use the given device address.
#[derive(Debug)]
pub struct SlaveClient<'client> {
    pub(crate) configured_address: u16,
    pub(crate) client: &'client Client<'client>,
}

impl<'client> SlaveClient<'client> {
    /// Create a new slave client instance.
    #[inline(always)]
    pub fn new(client: &'client Client<'client>, configured_address: u16) -> Self {
        Self {
            configured_address,
            client,
        }
    }

    /// Get configured timeouts.
    #[inline(always)]
    pub(crate) fn timeouts(&self) -> &Timeouts {
        &self.client.timeouts
    }

    #[inline(always)]
    pub(crate) async fn read_ignore_wkc<T>(&self, register: u16) -> Result<PduResponse<T>, Error>
    where
        T: EtherCrabWireReadSized,
    {
        Command::fprd(self.configured_address, register)
            .receive(self.client)
            .await
    }

    /// A wrapper around an FPWR service to this slave's configured address.
    #[inline(always)]
    pub(crate) async fn write_ignore_wkc<T>(
        &self,
        register: u16,
        value: T,
    ) -> Result<PduResponse<T>, Error>
    where
        T: EtherCrabWireReadWrite,
    {
        Command::fpwr(self.configured_address, register)
            .send_receive(self.client, value)
            .await
    }

    #[inline(always)]
    pub(crate) async fn read<T>(&self, register: u16, context: &'static str) -> Result<T, Error>
    where
        T: EtherCrabWireReadSized,
    {
        Command::fprd(self.configured_address, register)
            .receive(self.client)
            .await?
            .wkc(1, context)
    }

    #[inline(always)]
    pub(crate) async fn read_slice(
        &self,
        register: u16,
        len: u16,
        context: &'static str,
    ) -> Result<RxFrameDataBuf<'client>, Error> {
        Command::fprd(self.configured_address, register)
            .receive_slice(self.client, len)
            .await?
            .wkc(1, context)
    }

    #[inline(always)]
    pub(crate) async fn write_slice(
        &self,
        register: u16,
        value: &[u8],
        context: &'static str,
    ) -> Result<RxFrameDataBuf<'_>, Error> {
        Command::fpwr(self.configured_address, register)
            .send_receive_slice(self.client, value)
            .await?
            .wkc(1, context)
    }

    /// A wrapper around an FPWR service to this slave's configured address, ignoring any response.
    #[inline(always)]
    pub(crate) async fn write<T>(
        &self,
        register: u16,
        value: T,
        context: &'static str,
    ) -> Result<(), Error>
    where
        T: EtherCrabWireWrite,
    {
        Command::fpwr(self.configured_address, register)
            .send(self.client, value)
            .await?
            .wkc(1, context)
    }
}
