use crate::{
    eeprom::types::{CategoryType, SiiControl, SiiReadSize, SiiRequest},
    error::{EepromError, Error},
    register::RegisterAddress,
    slave::slave_client::SlaveClient,
};

/// The address of the first proper category, positioned after the fixed fields defined in ETG2010
/// Table 2.
const SII_FIRST_CATEGORY_START: u16 = 0x0040u16;

/// EEPROM section reader.
///
/// Controls an internal pointer to sequentially read data from a section in a slave's EEPROM.
pub struct EepromSectionReader<'a> {
    start: u16,
    /// Category length in bytes.
    len: u16,
    byte_count: u16,
    read: heapless::Deque<u8, 8>,
    // eeprom: &'a Eeprom<'a>,
    read_length: usize,
    client: &'a SlaveClient<'a>,
}

impl<'a> EepromSectionReader<'a> {
    /// Create a new EEPROM section reader.
    ///
    /// This is used to read data from individual sections in a slave's EEPROM. Many methods on
    /// `EepromSectionReader` will either return [`EepromError::SectionOverrun`] or
    /// [`EepromError::SectionUnderrun`] errors if the section cannot be completely read as this is
    /// often an indicator of a bug in either the slave's EEPROM or EtherCrab.
    pub async fn new(
        client: &'a SlaveClient<'a>,
        category: CategoryType,
    ) -> Result<Option<EepromSectionReader<'_>>, Error> {
        let mut start_word = SII_FIRST_CATEGORY_START;

        loop {
            let chunk = Self::read_eeprom_raw(client, start_word).await?;

            let category_type =
                CategoryType::from(u16::from_le_bytes(chunk[0..2].try_into().unwrap()));
            let len_words = u16::from_le_bytes(chunk[2..4].try_into().unwrap());

            // Position after header
            start_word += 2;

            log::trace!(
                "Found category {category_type:?}, data starts at {:#06x?}, length {:#04x?} ({}) bytes",
                start_word,
                len_words,
                len_words
            );

            match category_type {
                cat if cat == category => {
                    break Ok(Some(Self::start_at(client, start_word, len_words * 2)));
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
    pub fn start_at(client: &'a SlaveClient<'a>, address: u16, len_bytes: u16) -> Self {
        Self {
            start: address,
            len: len_bytes,
            byte_count: 0,
            read: heapless::Deque::new(),
            read_length: 0,
            client,
        }
    }

    async fn read_eeprom_raw<'sto>(
        client: &'sto SlaveClient<'sto>,
        eeprom_address: u16,
    ) -> Result<[u8; 8], Error> {
        let status = client
            .read::<SiiControl>(RegisterAddress::SiiControl, "Read SII control")
            .await?;

        // Clear errors
        if status.has_error() {
            log::trace!("Resetting EEPROM error flags");

            client
                .write(
                    RegisterAddress::SiiControl,
                    status.error_reset().as_array(),
                    "Reset errors",
                )
                .await?;
        }

        // Set up an SII read. This writes the control word and the register word after it
        // TODO: Consider either removing context strings or using defmt or something to avoid
        // bloat.
        client
            .write(
                RegisterAddress::SiiControl,
                SiiRequest::read(eeprom_address).as_array(),
                "SII read setup",
            )
            .await?;

        Self::wait(client).await?;

        let data = match status.read_size {
            // If slave uses 4 octet reads, do two reads so we can always return a chunk of 8 bytes
            SiiReadSize::Octets4 => {
                let chunk1 = client
                    .read::<[u8; 4]>(RegisterAddress::SiiData, "Read SII data")
                    .await?;

                // Move on to next chunk
                {
                    // NOTE: We must compute offset in 16 bit words, not bytes, hence the divide by 2
                    let setup = SiiRequest::read(eeprom_address + (chunk1.len() / 2) as u16);

                    client
                        .write(
                            RegisterAddress::SiiControl,
                            setup.as_array(),
                            "SII read setup",
                        )
                        .await?;

                    Self::wait(client).await?;
                }

                let chunk2 = client
                    .read::<[u8; 4]>(RegisterAddress::SiiData, "SII data 2")
                    .await?;

                let mut data = [0u8; 8];

                data[0..4].copy_from_slice(&chunk1);
                data[4..8].copy_from_slice(&chunk2);

                data
            }
            SiiReadSize::Octets8 => {
                client
                    .read::<[u8; 8]>(RegisterAddress::SiiData, "SII data")
                    .await?
            }
        };

        log::trace!("Read {:#04x?} {:02x?}", eeprom_address, data);

        Ok(data)
    }

    /// Wait for EEPROM read or write operation to finish and clear the busy flag.
    async fn wait<'sto>(client: &'sto SlaveClient<'sto>) -> Result<(), Error> {
        crate::timer_factory::timeout(client.timeouts().eeprom, async {
            loop {
                let control = client
                    .read::<SiiControl>(RegisterAddress::SiiControl, "SII busy wait")
                    .await?;

                if !control.busy {
                    break Ok(());
                }

                client.timeouts().loop_tick().await;
            }
        })
        .await
    }

    /// Read the next byte from the EEPROM.
    ///
    /// Internally, this method reads the EEPROM in chunks of 4 or 8 bytes (depending on the slave).
    pub async fn next(&mut self) -> Result<Option<u8>, Error> {
        if self.read.is_empty() {
            let read = Self::read_eeprom_raw(self.client, self.start).await?;

            let slice = read.as_slice();

            self.read_length = slice.len();

            for byte in slice.iter() {
                self.read.push_back(*byte).map_err(|_| {
                    log::error!("EEPROM read queue is full");

                    Error::Eeprom(EepromError::SectionOverrun)
                })?;
            }

            self.start += (self.read.len() / 2) as u16;
        }

        let result = self
            .read
            .pop_front()
            .filter(|_| self.byte_count < self.len)
            .map(|byte| {
                self.byte_count += 1;

                byte
            });

        Ok(result)
    }

    /// Skip a given number of addresses (note: not bytes).
    pub async fn skip(&mut self, skip: u16) -> Result<(), Error> {
        // TODO: Optimise by calculating new skip address instead of just iterating through chunks
        for _ in 0..skip {
            self.next().await?;
        }

        Ok(())
    }

    /// Try reading the next chunk in the current section.
    pub async fn try_next(&mut self) -> Result<u8, Error> {
        match self.next().await {
            Ok(Some(value)) => Ok(value),
            Ok(None) => Err(Error::Eeprom(EepromError::SectionOverrun)),
            Err(e) => Err(e),
        }
    }

    /// Attempt to read exactly `N` bytes. If not enough data could be read, this method returns an
    /// error.
    pub async fn take_vec_exact<const N: usize>(&mut self) -> Result<heapless::Vec<u8, N>, Error> {
        self.take_vec()
            .await?
            .ok_or(Error::Eeprom(EepromError::SectionUnderrun))
    }

    /// Read up to `N` bytes. If not enough data could be read, this method will return `Ok(None)`.
    pub async fn take_vec<const N: usize>(
        &mut self,
    ) -> Result<Option<heapless::Vec<u8, N>>, Error> {
        self.take_vec_len(N).await
    }

    /// Try to take `len` bytes, returning an error if the buffer length `N` is too small.
    ///
    /// If not enough data could be read, this method returns an error.
    pub async fn take_vec_len_exact<const N: usize>(
        &mut self,
        len: usize,
    ) -> Result<heapless::Vec<u8, N>, Error> {
        self.take_vec_len(len)
            .await?
            .ok_or(Error::Eeprom(EepromError::SectionUnderrun))
    }

    /// Try to take `len` bytes, returning an error if the buffer length `N` is too small.
    ///
    /// If not enough data can be read to fill the buffer, this method will return `Ok(None)`.
    pub async fn take_vec_len<const N: usize>(
        &mut self,
        len: usize,
    ) -> Result<Option<heapless::Vec<u8, N>>, Error> {
        let mut buf = heapless::Vec::new();

        let mut count = 0;

        log::trace!(
            "Taking bytes from EEPROM start {:#06x?}, len {}, N {}",
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
                log::error!("take_n_vec output buffer is full");

                break Err(Error::Eeprom(EepromError::SectionOverrun));
            }

            if let Some(byte) = self.next().await? {
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
