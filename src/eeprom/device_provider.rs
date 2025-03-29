use crate::{
    eeprom::{
        types::{SiiControl, SiiRequest},
        EepromDataProvider,
    },
    error::{EepromError, Error},
    fmt,
    register::RegisterAddress,
    timer_factory::IntoTimeout,
    Command, MainDevice,
};

/// The address of the first proper category, positioned after the fixed fields defined in ETG2010
/// Table 2.
///
/// SII EEPROM is WORD-addressed.
pub(crate) const SII_FIRST_CATEGORY_START: u16 = 0x0040u16;

/// EEPROM data provider that communicates with a physical sub device.
#[derive(Clone)]
pub struct DeviceEeprom<'subdevice> {
    maindevice: &'subdevice MainDevice<'subdevice>,
    configured_address: u16,
}

impl<'subdevice> DeviceEeprom<'subdevice> {
    /// Create a new EEPROM reader instance.
    pub fn new(maindevice: &'subdevice MainDevice<'subdevice>, configured_address: u16) -> Self {
        Self {
            maindevice,
            configured_address,
        }
    }

    async fn wait_while_busy(&self) -> Result<SiiControl, Error> {
        let res = async {
            loop {
                let control: SiiControl =
                    Command::fprd(self.configured_address, RegisterAddress::SiiControl.into())
                        .receive::<SiiControl>(self.maindevice)
                        .await?;

                if !control.busy {
                    break Ok(control);
                }

                self.maindevice.timeouts.loop_tick().await;
            }
        }
        .timeout(self.maindevice.timeouts.eeprom)
        .await?;

        Ok(res)
    }
}

impl<'subdevice> EepromDataProvider for DeviceEeprom<'subdevice> {
    async fn read_chunk(
        &mut self,
        start_word: u16,
    ) -> Result<impl core::ops::Deref<Target = [u8]>, Error> {
        Command::fpwr(self.configured_address, RegisterAddress::SiiControl.into())
            .send(self.maindevice, SiiRequest::read(start_word))
            .await?;

        let status = self.wait_while_busy().await?;

        Command::fprd(self.configured_address, RegisterAddress::SiiData.into())
            .receive_slice(self.maindevice, status.read_size.chunk_len())
            .await
            .map(|data| {
                #[cfg(not(feature = "defmt"))]
                fmt::trace!("Read addr {:#06x}: {:02x?}", start_word, &data[..]);
                #[cfg(feature = "defmt")]
                fmt::trace!("Read addr {:#06x}: {=[u8]}", start_word, &data[..]);

                data
            })
    }

    async fn write_word(&mut self, start_word: u16, data: [u8; 2]) -> Result<(), Error> {
        // Check if the EEPROM is busy
        self.wait_while_busy().await?;

        let mut retry_count = 0;

        loop {
            // Set data to write
            Command::fpwr(self.configured_address, RegisterAddress::SiiData.into())
                .send(self.maindevice, data)
                .await?;

            // Send control and address registers. A rising edge on the write flag will store whatever
            // is in `SiiAddress` into the EEPROM at the given address.
            Command::fpwr(self.configured_address, RegisterAddress::SiiControl.into())
                .send(self.maindevice, SiiRequest::write(start_word))
                .await?;

            // Wait for error or not busy
            let status = self.wait_while_busy().await?;

            if status.command_error && retry_count < 20 {
                fmt::debug!("Retrying EEPROM write");

                retry_count += 1;
            } else {
                break;
            }
        }

        Ok(())
    }

    async fn clear_errors(&self) -> Result<(), Error> {
        let status = Command::fprd(self.configured_address, RegisterAddress::SiiControl.into())
            .receive::<SiiControl>(self.maindevice)
            .await?;

        // Clear errors
        let status = if status.has_error() {
            fmt::trace!("Resetting EEPROM error flags");

            Command::fpwr(self.configured_address, RegisterAddress::SiiControl.into())
                .send_receive(self.maindevice, status.error_reset())
                .await?
        } else {
            status
        };

        if status.has_error() {
            Err(Error::Eeprom(EepromError::ClearErrors))
        } else {
            Ok(())
        }
    }
}
