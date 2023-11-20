use crate::{
    eeprom::{
        types::{CategoryType, SiiControl, SiiRequest},
        CategoryWrapper, EepromBlock, EepromDataProvider,
    },
    error::Error,
    fmt,
    pdu_loop::RxFrameDataBuf,
    register::RegisterAddress,
    slave::slave_client::SlaveClient,
};

/// The address of the first proper category, positioned after the fixed fields defined in ETG2010
/// Table 2.
const SII_FIRST_CATEGORY_START: u16 = 0x0040u16;

/// EEPROM section reader.
///
/// Controls an internal pointer to sequentially read data from a section in a slave's EEPROM.
pub struct EepromSectionReader<'slave> {
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
    read: heapless::Deque<u8, 8>,
    slave: &'slave SlaveClient<'slave>,
}

pub struct EepromFactory<'slave> {
    pub(crate) slave: &'slave SlaveClient<'slave>,
}

impl<'slave> EepromFactory<'slave> {
    pub fn new(slave: &'slave SlaveClient<'slave>) -> Self {
        Self { slave }
    }
}

impl<'slave> EepromDataProvider for EepromFactory<'slave> {
    type Handle = EepromSectionReader<'slave>;

    async fn category(
        &self,
        category: CategoryType,
    ) -> Result<Option<CategoryWrapper<Self::Handle>>, Error> {
        let r = EepromSectionReader::new(self.slave, category).await?;

        Ok(r.map(CategoryWrapper::new))
    }

    fn address(&self, address: u16, len_bytes: u16) -> CategoryWrapper<Self::Handle> {
        CategoryWrapper::new(EepromSectionReader::start_at(
            self.slave, address, len_bytes,
        ))
    }
}

impl<'slave> EepromSectionReader<'slave> {
    /// Create a new EEPROM section reader.
    ///
    /// This is used to read data from individual sections in a slave's EEPROM. Many methods on
    /// `EepromSectionReader` will either return [`EepromError::SectionOverrun`] or
    /// [`EepromError::SectionUnderrun`] errors if the section cannot be completely read as this is
    /// often an indicator of a bug in either the slave's EEPROM or EtherCrab.
    pub(in crate::eeprom) async fn new(
        slave: &'slave SlaveClient<'_>,
        category: CategoryType,
    ) -> Result<Option<Self>, Error> {
        let mut start_word = SII_FIRST_CATEGORY_START;

        loop {
            let chunk = Self::read_eeprom_raw(slave, start_word).await?;

            // The chunk is either 4 or 8 bytes long, so these unwraps should never fire.
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
                    break Ok(Some(Self::start_at(slave, start_word, len_words * 2)));
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
    pub(in crate::eeprom) fn start_at(
        slave: &'slave SlaveClient<'_>,
        address: u16,
        len_bytes: u16,
    ) -> Self {
        Self {
            start: address,
            len: len_bytes,
            byte_count: 0,
            read: heapless::Deque::new(),
            slave,
        }
    }

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

    // TODO: Get rid of this and read chunks directly into buffer
    async fn next(&mut self) -> Result<Option<u8>, Error> {
        if self.read.is_empty() {
            let read = Self::read_eeprom_raw(&self.slave, self.start).await?;

            for byte in read.iter() {
                // SAFETY:
                // - The queue is empty at this point
                // - The read chunk is 4 or 8 bytes long
                // - The queue has a capacity of 8 bytes
                // - So all 4 or 8 bytes will push into the 8 byte queue successfully
                unsafe { self.read.push_back_unchecked(*byte) };
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
}

impl<'slave> embedded_io_async::ErrorType for EepromSectionReader<'slave> {
    type Error = Error;
}

// TODO: Move to error.rs
impl embedded_io_async::Error for Error {
    fn kind(&self) -> embedded_io_async::ErrorKind {
        // TODO: match()?
        embedded_io_async::ErrorKind::Other
    }
}

impl<'slave> embedded_io_async::Read for EepromSectionReader<'slave> {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        // TODO: Read chunks instead of individual bytes

        let mut len = 0;

        while let Some(next) = self.next().await? {
            buf[len] = next;

            len += 1;
        }

        Ok(len)
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
