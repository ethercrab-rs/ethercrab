use embedded_io_async::SeekFrom;

use crate::{
    eeprom::{
        types::{SiiControl, SiiRequest},
        EepromDataProvider,
    },
    error::Error,
    fmt,
    pdu_loop::RxFrameDataBuf,
    register::RegisterAddress,
    slave::slave_client::SlaveClient,
};

/// The address of the first proper category, positioned after the fixed fields defined in ETG2010
/// Table 2.
pub(crate) const SII_FIRST_CATEGORY_START: u16 = 0x0040u16;

/// EEPROM data provider that communicates with a physical sub device.
pub struct SiiDataProvider<'slave> {
    client: &'slave SlaveClient<'slave>,
}

impl<'slave> SiiDataProvider<'slave> {
    pub fn new(client: &'slave SlaveClient<'slave>) -> Self {
        Self { client }
    }
}

impl<'slave> EepromDataProvider for SiiDataProvider<'slave> {
    type Provider = SiiDataProviderHandle<'slave>;

    fn reader(&self) -> Self::Provider {
        todo!()
    }
}

/// A sequential reader that reads bytes from a device's EEPROM.
pub struct SiiDataProviderHandle<'slave> {
    client: &'slave SlaveClient<'slave>,

    /// Current EEPROM address in WORDs.
    word_pos: u16,

    /// Internal cache used to store chunks read from device.
    read: heapless::Deque<u8, 8>,
}

impl<'slave> SiiDataProviderHandle<'slave> {
    async fn read_eeprom_raw<'client>(
        slave: &'client SlaveClient<'client>,
        eeprom_address: u16,
    ) -> Result<RxFrameDataBuf<'client>, Error> {
        let status = slave
            .read::<SiiControl>(RegisterAddress::SiiControl.into(), "Read SII control")
            .await?;

        // Clear errors
        if status.has_error() {
            fmt::trace!("Resetting EEPROM error flags");

            slave
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
        slave
            .write_slice(
                RegisterAddress::SiiControl.into(),
                &SiiRequest::read(eeprom_address).as_array(),
                "SII read setup",
            )
            .await?;

        wait(slave).await?;

        slave
            .read_slice(
                RegisterAddress::SiiData.into(),
                status.read_size.chunk_len(),
                "SII data",
            )
            .await
            .map(|data| {
                #[cfg(not(feature = "defmt"))]
                fmt::trace!("Read {:#04x} {:02x?}", eeprom_address, data);
                #[cfg(feature = "defmt")]
                fmt::trace!("Read {:#04x} {=[u8]}", eeprom_address, data);

                data
            })
    }

    async fn next(&mut self) -> Result<Option<u8>, Error> {
        if self.read.is_empty() {
            let read = Self::read_eeprom_raw(&self.client, self.word_pos).await?;

            for byte in read.iter() {
                // SAFETY:
                // - The queue is empty at this point
                // - The read chunk is 4 or 8 bytes long
                // - The queue has a capacity of 8 bytes
                // - So all 4 or 8 bytes will push into the 8 byte queue successfully
                unsafe { self.read.push_back_unchecked(*byte) };
            }

            self.word_pos += (self.read.len() / 2) as u16;
        }

        let result = self.read.pop_front();

        Ok(result)
    }
}

// impl<'slave> SiiDataProviderHandle<'slave> {
//     pub(in crate::eeprom) async fn new(client: &'slave SlaveClient<'slave>) -> Self {
//         Self {
//             client,
//             read: heapless::Deque::new(),
//             word_pos: 0,
//         }
//     }
// }

impl<'slave> embedded_io_async::Read for SiiDataProviderHandle<'slave> {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        let mut count = 0;

        // TODO: Optimise with chunks or whatever
        while count < buf.len() {
            let Some(byte) = self.next().await? else {
                return Ok(0);
            };

            buf[count] = byte;

            count += 1;
        }

        Ok(count)
    }
}

impl<'slave> embedded_io_async::Seek for SiiDataProviderHandle<'slave> {
    async fn seek(&mut self, pos: SeekFrom) -> Result<u64, Self::Error> {
        let SeekFrom::Start(pos) = pos else {
            panic!("Only support from start atm");
        };

        let pos = pos as u16;

        // TODO: Calculate offset instead of looping reads until we get to where we want to be.

        while self.word_pos * 2 < pos {
            //
        }

        todo!()
    }
}

impl<'slave> embedded_io_async::ErrorType for SiiDataProviderHandle<'slave> {
    type Error = Error;
}

// impl<'slave> embedded_io_async::Read for EepromSectionReader<'slave> {
//     async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
//         // TODO: Read chunks instead of individual bytes

//         let mut len = 0;

//         while let Some(next) = self.next().await? {
//             buf[len] = next;

//             len += 1;
//         }

//         Ok(len)
//     }
// }

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
