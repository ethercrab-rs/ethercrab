use crate::{
    eeprom::{
        types::{SiiControl, SiiRequest},
        EepromDataProvider,
    },
    error::Error,
    fmt,
    register::RegisterAddress,
    slave::slave_client::SlaveClient,
};

/// The address of the first proper category, positioned after the fixed fields defined in ETG2010
/// Table 2.
///
/// SII EEPROM is WORD-addressed.
pub(crate) const SII_FIRST_CATEGORY_START: u16 = 0x0040u16;

/// EEPROM data provider that communicates with a physical sub device.
#[derive(Clone)]
pub struct DeviceEeprom<'slave> {
    client: &'slave SlaveClient<'slave>,
}

impl<'slave> DeviceEeprom<'slave> {
    pub fn new(client: &'slave SlaveClient<'slave>) -> Self {
        Self { client }
    }
}

impl<'slave> EepromDataProvider for DeviceEeprom<'slave> {
    async fn read_chunk(
        &mut self,
        start_word: u16,
    ) -> Result<impl core::ops::Deref<Target = [u8]>, Error> {
        let status = self
            .client
            .read::<SiiControl>(RegisterAddress::SiiControl.into(), "Read SII control")
            .await?;

        // Clear errors
        if status.has_error() {
            fmt::trace!("Resetting EEPROM error flags");

            self.client
                .write_slice(
                    RegisterAddress::SiiControl.into(),
                    &status.error_reset().as_array(),
                    "Reset errors",
                )
                .await?;
        }

        // Set up an SII read. This writes the control word and the register word after it
        // TODO: Consider either removing context strings or using defmt or something to avoid
        // bloat.
        self.client
            .write_slice(
                RegisterAddress::SiiControl.into(),
                &SiiRequest::read(start_word).as_array(),
                "SII read setup",
            )
            .await?;

        wait(self.client).await?;

        self.client
            .read_slice(
                RegisterAddress::SiiData.into(),
                status.read_size.chunk_len(),
                "SII data",
            )
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

/// Wait for EEPROM read or write operation to finish and clear the busy flag.
async fn wait(slave: &SlaveClient<'_>) -> Result<(), Error> {
    crate::timer_factory::timeout(slave.timeouts().eeprom, async {
        loop {
            let control = slave
                .read::<SiiControl>(RegisterAddress::SiiControl.into(), "SII busy wait")
                .await?;

            if !control.busy {
                break Ok(());
            }

            slave.timeouts().loop_tick().await;
        }
    })
    .await
}
