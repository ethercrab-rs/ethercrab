use crate::{
    eeprom::types::{CategoryType, SiiControl, SiiReadSize, SiiRequest},
    error::{EepromError, Error},
    fmt,
    register::RegisterAddress,
    slave::slave_client::SlaveClient,
};
use core::array::IntoIter;

/// The address of the first proper category, positioned after the fixed fields defined in ETG2010
/// Table 2.
const SII_FIRST_CATEGORY_START: u16 = 0x0040u16;

/// EEPROM section reader.
///
/// Controls an internal pointer to sequentially read data from a section in a slave's EEPROM.
pub struct EepromSectionReader {
    /// Start address.
    ///
    /// EEPROM is structured as 16 bit words, so address strides must be halved to step correctly.
    start: u16,

    /// Category length in bytes.
    ///
    /// This is the maximum number of bytes this `Reader` instance will return.
    len: u16,

    /// Number of bytes read so far.
    byte_count: u16,

    // MSRV: Get rid of Option when `array_into_iter_constructors` is stablised. Track here:
    // <https://github.com/rust-lang/rust/issues/91583>.
    read: Option<IntoIter<u8, 8>>,
}

impl EepromSectionReader {
    /// Create a new EEPROM section reader.
    ///
    /// This is used to read data from individual sections in a slave's EEPROM. Many methods on
    /// `EepromSectionReader` will either return [`EepromError::SectionOverrun`] or
    /// [`EepromError::SectionUnderrun`] errors if the section cannot be completely read as this is
    /// often an indicator of a bug in either the slave's EEPROM or EtherCrab.
    pub async fn new(
        slave: &SlaveClient<'_>,
        category: CategoryType,
    ) -> Result<Option<Self>, Error> {
        let mut start_word = SII_FIRST_CATEGORY_START;

        loop {
            let chunk = Self::read_eeprom_raw(slave, start_word).await?;

            let category_type =
                CategoryType::from(u16::from_le_bytes(fmt::unwrap!(chunk[0..2].try_into())));
            let len_words = u16::from_le_bytes(fmt::unwrap!(chunk[2..4].try_into()));

            // Position after header
            start_word += 2;

            fmt::trace!(
                "Found category {:?}, data starts at {:#06x}, length {:#04x} ({}) bytes",
                category_type,
                start_word,
                len_words,
                len_words
            );

            match category_type {
                cat if cat == category => {
                    break Ok(Some(Self::start_at(start_word, len_words * 2)));
                }
                CategoryType::End => break Ok(None),
                _ => (),
            }

            // Next category starts after the current category's data
            start_word += len_words;
        }
    }

    /// Read an arbitrary chunk of the EEPROM instead of using an EEPROM section configu to define
    /// start address and length.
    pub fn start_at(address: u16, len_bytes: u16) -> Self {
        Self {
            len: len_bytes,
            read: None,
            byte_count: 0,
            start: address,
        }
    }

    async fn read_eeprom_raw(
        slave: &SlaveClient<'_>,
        eeprom_address: u16,
    ) -> Result<[u8; 8], Error> {
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
        slave
            .write_slice(
                RegisterAddress::SiiControl.into(),
                &SiiRequest::read(eeprom_address).as_array(),
                "SII read setup",
            )
            .await?;

        Self::wait(slave).await?;

        let data = match status.read_size {
            // If slave uses 4 octet reads, do two reads so we can always return a chunk of 8 bytes
            SiiReadSize::Octets4 => {
                let mut data = [0u8; 8];

                let chunk = slave
                    .read_slice(RegisterAddress::SiiData.into(), 4, "Read SII data")
                    .await?;

                data[0..4].copy_from_slice(&chunk);

                // Move on to next chunk
                {
                    // NOTE: We must compute offset in 16 bit words, not bytes, hence the divide by 2
                    let setup = SiiRequest::read(eeprom_address + (data.len() / 2) as u16);

                    slave
                        .write_slice(
                            RegisterAddress::SiiControl.into(),
                            &setup.as_array(),
                            "SII read setup",
                        )
                        .await?;

                    Self::wait(slave).await?;
                }

                let chunk2 = slave
                    .read_slice(RegisterAddress::SiiData.into(), 4, "SII data 2")
                    .await?;

                // fmt::unwrap!(data.extend_from_slice(&chunk2));
                data[4..8].copy_from_slice(&chunk2);

                data
            }
            SiiReadSize::Octets8 => {
                slave
                    .read(RegisterAddress::SiiData.into(), "SII data")
                    .await?
            }
        };

        #[cfg(not(feature = "defmt"))]
        fmt::trace!("Read {:#04x} {:02x?}", eeprom_address, data);
        #[cfg(feature = "defmt")]
        fmt::trace!("Read {:#04x} {=[u8]}", eeprom_address, data);

        Ok(data)
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

    /// Read the next byte from the EEPROM.
    ///
    /// Internally, this method reads the EEPROM in chunks of 4 or 8 bytes (depending on the slave).
    pub async fn next(&mut self, slave: &SlaveClient<'_>) -> Result<Option<u8>, Error> {
        // Reached end of section
        if self.byte_count >= self.len {
            return Ok(None);
        }

        let next = self.read.as_mut().and_then(|r| r.next());

        let next = if let Some(next) = next {
            next
        } else {
            let read = Self::read_eeprom_raw(slave, self.start).await?;

            self.read = Some(read.into_iter());

            // Step ahead to next address. Addresses are in words, so we divide the read length by
            // 2.
            self.start += (read.len() / 2) as u16;

            // This won't panic as we just filled the iterator
            fmt::unwrap_opt!(self.read.as_mut().and_then(|r| r.next()))
        };

        self.byte_count += 1;

        Ok(Some(next))
    }

    /// Skip a given number of addresses (note: not bytes).
    pub async fn skip(&mut self, slave: &SlaveClient<'_>, skip: u16) -> Result<(), Error> {
        // TODO: Optimise by calculating new skip address instead of just iterating through chunks
        for _ in 0..skip {
            self.next(slave).await?;
        }

        Ok(())
    }

    /// Try reading the next chunk in the current section.
    pub async fn try_next(&mut self, slave: &SlaveClient<'_>) -> Result<u8, Error> {
        match self.next(slave).await? {
            Some(value) => Ok(value),
            None => Err(Error::Eeprom(EepromError::SectionOverrun)),
        }
    }

    /// Attempt to read exactly `N` bytes. If not enough data could be read, this method returns an
    /// error.
    pub async fn take_vec_exact<const N: usize>(
        &mut self,
        slave: &SlaveClient<'_>,
    ) -> Result<heapless::Vec<u8, N>, Error> {
        self.take_vec(slave)
            .await?
            .ok_or(Error::Eeprom(EepromError::SectionUnderrun))
    }

    /// Read up to `N` bytes. If not enough data could be read, this method will return `Ok(None)`.
    pub async fn take_vec<const N: usize>(
        &mut self,
        slave: &SlaveClient<'_>,
    ) -> Result<Option<heapless::Vec<u8, N>>, Error> {
        self.take_vec_len(slave, N).await
    }

    /// Try to take `len` bytes, returning an error if the buffer length `N` is too small.
    ///
    /// If not enough data could be read, this method returns an error.
    pub async fn take_vec_len_exact<const N: usize>(
        &mut self,
        slave: &SlaveClient<'_>,
        len: usize,
    ) -> Result<heapless::Vec<u8, N>, Error> {
        self.take_vec_len(slave, len)
            .await?
            .ok_or(Error::Eeprom(EepromError::SectionUnderrun))
    }

    /// Try to take `len` bytes, returning an error if the buffer length `N` is too small.
    ///
    /// If not enough data can be read to fill the buffer, this method will return `Ok(None)`.
    pub async fn take_vec_len<const N: usize>(
        &mut self,
        slave: &SlaveClient<'_>,
        len: usize,
    ) -> Result<Option<heapless::Vec<u8, N>>, Error> {
        let mut buf = heapless::Vec::new();

        let mut count = 0;

        fmt::trace!(
            "Taking bytes from EEPROM start {:#06x}, len {}, N {}",
            self.start,
            len,
            N
        );

        loop {
            // We've collected the requested number of bytes
            if count >= len {
                break Ok(Some(buf));
            }

            // If buffer is full, we'd end up with truncated data, so error out.
            if buf.is_full() {
                fmt::error!("take_n_vec output buffer is full");

                break Err(Error::Eeprom(EepromError::SectionOverrun));
            }

            if let Some(byte) = self.next(slave).await? {
                // SAFETY: We check for buffer space using is_full above
                unsafe { buf.push_unchecked(byte) };

                count += 1;
            } else {
                // Not enough data to fill the buffer
                break Ok(None);
            }
        }
    }
}
