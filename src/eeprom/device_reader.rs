use crate::{
    eeprom::{
        types::{SiiControl, SiiRequest},
        EepromDataProvider,
    },
    error::Error,
    fmt,
    register::RegisterAddress,
    SlaveRef,
};

/// The address of the first proper category, positioned after the fixed fields defined in ETG2010
/// Table 2.
///
/// SII EEPROM is WORD-addressed.
pub(crate) const SII_FIRST_CATEGORY_START: u16 = 0x0040u16;

/// EEPROM data provider that communicates with a physical sub device.
#[derive(Clone)]
pub struct DeviceEeprom<'slave> {
    client: SlaveRef<'slave, ()>,
}

impl<'slave> DeviceEeprom<'slave> {
    pub fn new(client: SlaveRef<'slave, ()>) -> Self {
        Self { client }
    }

    /// Wait for EEPROM read or write operation to finish and clear the busy flag.
    async fn wait(&self) -> Result<(), Error> {
        crate::timer_factory::timeout(self.client.timeouts().eeprom, async {
            loop {
                let control = self
                    .client
                    .read(RegisterAddress::SiiControl)
                    .receive::<SiiControl>()
                    .await?;

                if !control.busy {
                    break Ok(());
                }

                self.client.timeouts().loop_tick().await;
            }
        })
        .await
    }
}

impl<'slave> EepromDataProvider for DeviceEeprom<'slave> {
    async fn read_chunk(
        &mut self,
        start_word: u16,
    ) -> Result<impl core::ops::Deref<Target = [u8]>, Error> {
        let status = self
            .client
            .read(RegisterAddress::SiiControl)
            .receive::<SiiControl>()
            .await?;

        // Clear errors
        if status.has_error() {
            fmt::trace!("Resetting EEPROM error flags");

            self.client
                .write(RegisterAddress::SiiControl)
                .send(status.error_reset())
                .await?;
        }

        self.client
            .write(RegisterAddress::SiiControl)
            .send_receive(SiiRequest::read(start_word))
            .await?;

        self.wait().await?;

        self.client
            .read(RegisterAddress::SiiData)
            .receive_slice(status.read_size.chunk_len())
            .await
            .map(|data| {
                #[cfg(not(feature = "defmt"))]
                fmt::trace!("Read addr {:#06x}: {:02x?}", start_word, data);
                #[cfg(feature = "defmt")]
                fmt::trace!("Read addr {:#06x}: {=[u8]}", start_word, data);

                data
            })
    }
}
