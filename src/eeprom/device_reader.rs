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
use embedded_io_async::SeekFrom;

// use super::ChunkReaderTraitLol;

/// The address of the first proper category, positioned after the fixed fields defined in ETG2010
/// Table 2.
///
/// SII EEPROM is WORD-addressed.
pub(crate) const SII_FIRST_CATEGORY_START: u16 = 0x0040u16;

/// EEPROM data provider that communicates with a physical sub device.
#[derive(Clone)]
pub struct SiiDataProvider<'slave> {
    client: &'slave SlaveClient<'slave>,
}

impl<'slave> SiiDataProvider<'slave> {
    /// Create a new provider.
    ///
    /// Call `reader` to get a handle to read/seek the EEPROM.
    pub fn new(client: &'slave SlaveClient<'slave>) -> Self {
        Self { client }
    }
}

impl<'slave> EepromDataProvider for SiiDataProvider<'slave> {
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

/// A sequential reader that reads bytes from a device's EEPROM.
pub struct SiiDataProviderHandle<'slave> {
    client: &'slave SlaveClient<'slave>,
    // /// Current EEPROM address in WORDs.
    // word_pos: u16,

    // /// Internal cache used to store chunks read from device.
    // read: heapless::Deque<u8, 8>,
}

// impl<'slave> core::fmt::Debug for SiiDataProviderHandle<'slave> {
//     fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
//         f.debug_struct("SiiDataProviderHandle")
//             .field("word_pos", &self.word_pos)
//             .field("read", &self.read)
//             .finish()
//     }
// }

// impl<'slave> SiiDataProviderHandle<'slave> {
//     pub(in crate::eeprom) fn new(client: &'slave SlaveClient<'slave>) -> Self {
//         Self {
//             client,
//             // read: heapless::Deque::new(),
//             // read2: heapless::Vec::new(),
//             // word_pos: 0,
//         }
//     }
// }

// impl<'slave> ChunkReaderTraitLol for SiiDataProviderHandle<'slave> {
//     async fn read_chunk(
//         &mut self,
//         start_word: u16,
//     ) -> Result<impl core::ops::Deref<Target = [u8]>, Error> {
//         let status = self
//             .client
//             .read::<SiiControl>(RegisterAddress::SiiControl.into(), "Read SII control")
//             .await?;

//         // Clear errors
//         if status.has_error() {
//             fmt::trace!("Resetting EEPROM error flags");

//             self.client
//                 .write_slice(
//                     RegisterAddress::SiiControl.into(),
//                     &status.error_reset().as_array(),
//                     "Reset errors",
//                 )
//                 .await?;
//         }

//         // Set up an SII read. This writes the control word and the register word after it
//         // TODO: Consider either removing context strings or using defmt or something to avoid
//         // bloat.
//         self.client
//             .write_slice(
//                 RegisterAddress::SiiControl.into(),
//                 &SiiRequest::read(start_word).as_array(),
//                 "SII read setup",
//             )
//             .await?;

//         wait(self.client).await?;

//         self.client
//             .read_slice(
//                 RegisterAddress::SiiData.into(),
//                 status.read_size.chunk_len(),
//                 "SII data",
//             )
//             .await
//             .map(|data| {
//                 #[cfg(not(feature = "defmt"))]
//                 fmt::trace!("Read addr {:#06x}: {:02x?}", start_word, data);
//                 #[cfg(feature = "defmt")]
//                 fmt::trace!("Read addr {:#06x}: {=[u8]}", eeprom_address, data);

//                 data
//             })
//     }

//     // // TODO: Refactor file reader to give slices of either 4 or 8 bytes (configurable for testing
//     // // purposes!)
//     // //
//     // // The read/seek impls move up a layer, and read_eeprom_raw becomes "read_chunk", along with a "seek" method. These are both methods on a `ChunkReader` trait.
//     // //
//     // // The `Provider` associated type on `EepromDataProvider` then uses this trait as a bound instead of the Read + Seek + etc
//     // //
//     // // All we're doing is abstracting over a reader/seeker impl with something that returns slices of fixed lengths, right?
//     // async fn read_eeprom_raw<'client>(
//     //     slave: &'client SlaveClient<'client>,
//     //     eeprom_address: u16,
//     // ) -> Result<impl core::ops::Deref<Target = [u8]> + 'client, Error> {
//     //     let status = slave
//     //         .read::<SiiControl>(RegisterAddress::SiiControl.into(), "Read SII control")
//     //         .await?;

//     //     // Clear errors
//     //     if status.has_error() {
//     //         fmt::trace!("Resetting EEPROM error flags");

//     //         slave
//     //             .write_slice(
//     //                 RegisterAddress::SiiControl.into(),
//     //                 &status.error_reset().as_array(),
//     //                 "Reset errors",
//     //             )
//     //             .await?;
//     //     }

//     //     // Set up an SII read. This writes the control word and the register word after it
//     //     // TODO: Consider either removing context strings or using defmt or something to avoid
//     //     // bloat.
//     //     slave
//     //         .write_slice(
//     //             RegisterAddress::SiiControl.into(),
//     //             &SiiRequest::read(eeprom_address).as_array(),
//     //             "SII read setup",
//     //         )
//     //         .await?;

//     //     wait(slave).await?;

//     //     slave
//     //         .read_slice(
//     //             RegisterAddress::SiiData.into(),
//     //             status.read_size.chunk_len(),
//     //             "SII data",
//     //         )
//     //         .await
//     //         .map(|data| {
//     //             #[cfg(not(feature = "defmt"))]
//     //             fmt::trace!("Read addr {:#06x}: {:02x?}", eeprom_address, data);
//     //             #[cfg(feature = "defmt")]
//     //             fmt::trace!("Read addr {:#06x}: {=[u8]}", eeprom_address, data);

//     //             data
//     //         })
//     // }

//     // async fn next(&mut self) -> Result<Option<u8>, Error> {
//     //     if self.read.is_empty() {
//     //         let read = Self::read_eeprom_raw(self.client, self.word_pos).await?;

//     //         for byte in read.iter() {
//     //             // SAFETY:
//     //             // - The queue is empty at this point
//     //             // - The read chunk is 4 or 8 bytes long
//     //             // - The queue has a capacity of 8 bytes
//     //             // - So all 4 or 8 bytes will push into the 8 byte queue successfully
//     //             unsafe { self.read.push_back_unchecked(*byte) };
//     //         }

//     //         self.word_pos += (self.read.len() / 2) as u16;
//     //     }

//     //     let result = self.read.pop_front();

//     //     Ok(result)
//     // }
// }

// impl<'slave> embedded_io_async::Read for SiiDataProviderHandle<'slave> {
//     async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
//         let mut count = 0;

//         // let cached_len = dbg!(self.read2.len().min(buf.len()));

//         // let rest = if let Some(cached) = self.read2.get(0..cached_len) {
//         //     dbg!(cached);

//         //     let (buf, rest) = buf.split_at_mut(cached.len());

//         //     buf.copy_from_slice(cached);

//         //     count += cached_len;

//         //     dbg!(count);

//         //     rest
//         // } else {
//         //     buf
//         // };

//         // dbg!(rest);

//         // if !rest.is_empty() {
//         //     //
//         // }

//         // While buffer has space for full slices, fill it up
//         // Need to take any remaining bytes in the internal buffer first

//         // The final write into the buffer will be a full read from the EEPROM but have remaining bytes

//         // Store remaining bytes in internal buffer

//         // TODO: Optimise with chunks or whatever
//         while count < buf.len() {
//             let Some(byte) = self.next().await? else {
//                 return Ok(0);
//             };

//             buf[count] = byte;

//             count += 1;
//         }

//         Ok(count)
//     }
// }

// impl<'slave> embedded_io_async::Seek for SiiDataProviderHandle<'slave> {
//     /// Seek to a WORD address (NOT bytes).
//     async fn seek(&mut self, offset: SeekFrom) -> Result<u64, Self::Error> {
//         // Target WORD position
//         let pos = match offset {
//             SeekFrom::Start(addr) => addr,
//             SeekFrom::End(_) => fmt::panic!("From end not supported"),
//             SeekFrom::Current(offset) => {
//                 // Current position + desired offset - anything left in the buffer (/ 2 to get word
//                 // count from bytes). The subtraction of the buffer length is required because a
//                 // read increments the address pointer by the returned data length, even if not all
//                 // of it is used.
//                 u64::from(self.word_pos)
//                     + fmt::unwrap!(u64::try_from(offset), "Negative offsets not supported")
//                     // FIXME: What happens when the buffer is an odd length?
//                     - (self.read.len()as u64  + 1) / 2
//             }
//         };

//         fmt::trace!(
//             "Seek {:?}, current pos {:#06x}, to {:#06x}",
//             offset,
//             self.word_pos,
//             pos
//         );

//         debug_assert!(pos <= u64::from(u16::MAX));

//         // Reset state so next read picks up from the new position we just calculated.
//         self.word_pos = pos as u16;
//         self.read.clear();

//         Ok(pos)
//     }
// }

// impl<'slave> embedded_io_async::ErrorType for SiiDataProviderHandle<'slave> {
//     type Error = Error;
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
