use crate::{
    error::Error,
    pdu_data::{PduData, PduRead},
    pdu_loop::{CheckWorkingCounter, PduResponse, RxFrameDataBuf},
    Client, Command, Timeouts,
};

#[derive(Debug)]
pub struct SlaveClient<'client> {
    pub configured_address: u16,
    pub client: &'client Client<'client>,
}

impl<'client> SlaveClient<'client> {
    pub(crate) fn new(client: &'client Client<'client>, configured_address: u16) -> Self {
        Self {
            configured_address,
            client,
        }
    }

    pub fn timeouts(&self) -> &Timeouts {
        &self.client.timeouts
    }

    pub(crate) async fn read_ignore_wkc<T>(&self, register: u16) -> Result<PduResponse<T>, Error>
    where
        T: PduRead,
    {
        Command::fprd(self.configured_address, register)
            .receive(&self.client)
            .await
    }

    /// A wrapper around an FPWR service to this slave's configured address.
    pub(crate) async fn write_ignore_wkc<T>(
        &self,
        register: u16,
        value: T,
    ) -> Result<PduResponse<T>, Error>
    where
        T: PduData,
    {
        Command::fpwr(self.configured_address, register)
            .send_receive(&self.client, value)
            .await
    }

    pub(crate) async fn read<T>(&self, register: u16, context: &'static str) -> Result<T, Error>
    where
        T: PduRead,
    {
        Command::fprd(self.configured_address, register)
            .receive(&self.client)
            .await?
            .wkc(1, context)
    }

    pub(crate) async fn read_slice(
        &self,
        register: u16,
        len: u16,
        context: &'static str,
    ) -> Result<RxFrameDataBuf<'_>, Error> {
        Command::fprd(self.configured_address, register)
            .receive_slice(&self.client, len)
            .await?
            .wkc(1, context)
    }

    pub(crate) async fn write_slice(
        &self,
        register: u16,
        value: &[u8],
        context: &'static str,
    ) -> Result<RxFrameDataBuf<'_>, Error> {
        Command::fpwr(self.configured_address, register)
            .send_receive_slice(&self.client, value)
            .await?
            .wkc(1, context)
    }

    /// A wrapper around an FPWR service to this slave's configured address, ignoring any response.
    pub(crate) async fn write<T>(
        &self,
        register: u16,
        value: T,
        context: &'static str,
    ) -> Result<(), Error>
    where
        T: PduData,
    {
        Command::fpwr(self.configured_address, register)
            .send(&self.client, value)
            .await?
            .wkc(1, context)
    }
}
