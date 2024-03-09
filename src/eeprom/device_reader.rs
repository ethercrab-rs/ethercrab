use crate::{
    eeprom::{
        types::{SiiControl, SiiRequest},
        EepromDataProvider,
    },
    error::{EepromError, Error},
    fmt,
    register::RegisterAddress,
    timer_factory::IntoTimeout,
    Client, Command,
};

/// The address of the first proper category, positioned after the fixed fields defined in ETG2010
/// Table 2.
///
/// SII EEPROM is WORD-addressed.
pub(crate) const SII_FIRST_CATEGORY_START: u16 = 0x0040u16;

/// EEPROM data provider that communicates with a physical sub device.
#[derive(Clone)]
pub struct DeviceEeprom<'slave> {
    client: &'slave Client<'slave>,
    configured_address: u16,
}

impl<'slave> DeviceEeprom<'slave> {
    /// Create a new EEPROM reader instance.
    pub fn new(client: &'slave Client<'slave>, configured_address: u16) -> Self {
        Self {
            client,
            configured_address,
        }
    }
}

impl<'slave> EepromDataProvider for DeviceEeprom<'slave> {
    async fn read_chunk(
        &mut self,
        start_word: u16,
    ) -> Result<impl core::ops::Deref<Target = [u8]>, Error> {
        Command::fpwr(self.configured_address, RegisterAddress::SiiControl.into())
            .send_receive(self.client, SiiRequest::read(start_word))
            .await?;

        let status = async {
            loop {
                let control: SiiControl =
                    Command::fprd(self.configured_address, RegisterAddress::SiiControl.into())
                        .receive::<SiiControl>(self.client)
                        .await?;

                if !control.busy {
                    break Ok(control);
                }

                self.client.timeouts.loop_tick().await;
            }
        }
        .timeout(self.client.timeouts.eeprom)
        .await?;

        Command::fprd(self.configured_address, RegisterAddress::SiiData.into())
            .receive_slice(self.client, status.read_size.chunk_len())
            .await
            .map(|data| {
                #[cfg(not(feature = "defmt"))]
                fmt::trace!("Read addr {:#06x}: {:02x?}", start_word, data);
                #[cfg(feature = "defmt")]
                fmt::trace!("Read addr {:#06x}: {=[u8]}", start_word, data);

                data
            })
    }

    async fn clear_errors(&self) -> Result<(), Error> {
        let status = Command::fprd(self.configured_address, RegisterAddress::SiiControl.into())
            .receive::<SiiControl>(self.client)
            .await?;

        // Clear errors
        if status.has_error() {
            fmt::trace!("Resetting EEPROM error flags");

            Command::fpwr(self.configured_address, RegisterAddress::SiiControl.into())
                .send(self.client, status.error_reset())
                .await?;
        }

        let status = Command::fprd(self.configured_address, RegisterAddress::SiiControl.into())
            .receive::<SiiControl>(self.client)
            .await?;

        if status.has_error() {
            Err(Error::Eeprom(EepromError::ClearErrors))
        } else {
            Ok(())
        }
    }
}
