use crate::{error::Error, pdu_loop::RxFrameDataBuf, Client, Command, Timeouts};
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
    pub(crate) async fn read_ignore_wkc<T>(&self, register: u16) -> Result<T, Error>
    where
        T: EtherCrabWireReadSized,
    {
        Command::fprd(self.configured_address, register)
            .wrap(self.client)
            .ignore_wkc()
            .receive()
            .await
    }

    #[inline(always)]
    pub(crate) async fn read<T>(&self, register: u16) -> Result<T, Error>
    where
        T: EtherCrabWireReadSized,
    {
        Command::fprd(self.configured_address, register)
            .wrap(self.client)
            .with_wkc(1)
            .receive()
            .await
    }

    #[inline(always)]
    pub(crate) async fn read_slice(
        &self,
        register: u16,
        len: u16,
    ) -> Result<RxFrameDataBuf<'client>, Error> {
        Command::fprd(self.configured_address, register)
            .wrap(self.client)
            .with_wkc(1)
            .receive_slice(len)
            .await
    }

    #[inline(always)]
    pub(crate) async fn write_slice(
        &self,
        register: u16,
        value: &[u8],
    ) -> Result<RxFrameDataBuf<'_>, Error> {
        Command::fpwr(self.configured_address, register)
            .wrap(self.client)
            .with_wkc(1)
            .send_receive_slice(value)
            .await
    }

    /// A wrapper around an FPWR service to this slave's configured address, ignoring any response.
    #[inline(always)]
    pub(crate) async fn write<T>(&self, register: u16, value: T) -> Result<(), Error>
    where
        T: EtherCrabWireWrite,
    {
        Command::fpwr(self.configured_address, register)
            .wrap(self.client)
            .with_wkc(1)
            .send(value)
            .await
    }

    #[inline(always)]
    pub(crate) async fn write_read<T>(&self, register: u16, value: T) -> Result<T, Error>
    where
        T: EtherCrabWireReadWrite,
    {
        Command::fpwr(self.configured_address, register)
            .wrap(self.client)
            .with_wkc(1)
            .send_receive(value)
            .await
    }
}
