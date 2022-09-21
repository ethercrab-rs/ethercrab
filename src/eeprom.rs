use core::{fmt, mem, str::FromStr};

use crate::{
    client::Client,
    error::Error,
    fmmu::Fmmu,
    pdu::CheckWorkingCounter,
    register::RegisterAddress,
    sii::{
        CategoryType, SiiCategory, SiiCoding, SiiControl, SiiGeneral, SiiReadSize, SiiRequest,
        SyncManager,
    },
    timer_factory::TimerFactory,
    PduRead,
};
use nom::multi::length_data;
use nom::number::complete::le_u8;
use pcap::sendqueue::Sync;

const SII_FIRST_SECTION_START: u16 = 0x0040u16;

enum EepromRead {
    Bytes4([u8; 4]),
    Bytes8([u8; 8]),
}

impl EepromRead {
    fn as_slice(&self) -> &[u8] {
        match self {
            EepromRead::Bytes4(arr) => arr.as_slice(),
            EepromRead::Bytes8(arr) => arr.as_slice(),
        }
    }

    fn bytes4(&self) -> [u8; 4] {
        match self {
            EepromRead::Bytes4(arr) => *arr,
            // TODO: Use `array_chunks` or similar method when stabilised.
            EepromRead::Bytes8(arr) => [arr[0], arr[1], arr[2], arr[3]],
        }
    }
}

impl fmt::Debug for EepromRead {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Bytes4(arg0) => write!(f, "{:02x?}", arg0),
            Self::Bytes8(arg0) => write!(f, "{:02x?}", arg0),
        }
    }
}

impl From<[u8; 4]> for EepromRead {
    fn from(arr: [u8; 4]) -> Self {
        Self::Bytes4(arr)
    }
}

impl From<[u8; 8]> for EepromRead {
    fn from(arr: [u8; 8]) -> Self {
        Self::Bytes8(arr)
    }
}

struct EepromSectionReader<
    'a,
    const MAX_FRAMES: usize,
    const MAX_PDU_DATA: usize,
    const MAX_SLAVES: usize,
    TIMEOUT,
> {
    start: u16,
    len: u16,
    byte_count: u16,
    read: heapless::Deque<u8, 8>,
    eeprom: &'a Eeprom<'a, MAX_FRAMES, MAX_PDU_DATA, MAX_SLAVES, TIMEOUT>,
    read_length: usize,
}

impl<'a, const MAX_FRAMES: usize, const MAX_PDU_DATA: usize, const MAX_SLAVES: usize, TIMEOUT>
    EepromSectionReader<'a, MAX_FRAMES, MAX_PDU_DATA, MAX_SLAVES, TIMEOUT>
where
    TIMEOUT: TimerFactory,
{
    fn new(
        eeprom: &'a Eeprom<'a, MAX_FRAMES, MAX_PDU_DATA, MAX_SLAVES, TIMEOUT>,
        cat: SiiCategory,
    ) -> Self {
        Self {
            eeprom,
            start: cat.start,
            // Category length is given in words (u16) but we're counting bytes here.
            len: cat.len_words * 2,
            byte_count: 0,
            read: heapless::Deque::new(),
            read_length: 0,
        }
    }

    async fn next(&mut self) -> Result<Option<u8>, Error> {
        if self.read.is_empty() {
            let read = self.eeprom.read_eeprom_raw(self.start).await?;

            let slice = read.as_slice();

            self.read_length = slice.len();

            for byte in slice.iter() {
                self.read
                    .push_back(*byte)
                    .map_err(|_| Error::EepromSectionOverrun)?;
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

    async fn skip(&mut self, skip: u16) -> Result<(), Error> {
        // TODO: Optimise by calculating new skip address instead of just iterating through chunks
        for _ in 0..skip {
            self.next().await?;
        }

        Ok(())
    }

    async fn try_next(&mut self) -> Result<u8, Error> {
        match self.next().await {
            Ok(Some(value)) => Ok(value),
            // TODO: New error type
            Ok(None) => Err(Error::EepromSectionOverrun),
            Err(e) => Err(e),
        }
    }

    async fn take_vec<const N: usize>(&mut self) -> Result<Option<heapless::Vec<u8, N>>, Error> {
        self.take_n_vec(N).await
    }

    async fn take_vec_exact<const N: usize>(&mut self) -> Result<heapless::Vec<u8, N>, Error> {
        self.take_n_vec(N)
            .await?
            .ok_or_else(|| Error::EepromSectionUnderrun)
    }

    async fn take_n_vec_exact<const N: usize>(
        &mut self,
        len: usize,
    ) -> Result<heapless::Vec<u8, N>, Error> {
        self.take_n_vec(len)
            .await?
            .ok_or_else(|| Error::EepromSectionUnderrun)
    }

    /// Try to take `len` bytes, returning an error if the buffer length `N` is too small.
    async fn take_n_vec<const N: usize>(
        &mut self,
        len: usize,
    ) -> Result<Option<heapless::Vec<u8, N>>, Error> {
        let mut buf = heapless::Vec::new();

        let mut count = 0;

        log::trace!(
            "Taking bytes from EEPROM start {}, len {}, N {}",
            self.start,
            len,
            N
        );

        // TODO: Optimise by taking chunks instead of calling next().await until end conditions are satisfied
        loop {
            // We've collected the requested number of bytes
            if count >= len {
                break Ok(Some(buf));
            }

            // If buffer is full, we'd end up with truncated data, so error out.
            if buf.is_full() {
                break Err(Error::EepromSectionOverrun);
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

pub struct Eeprom<
    'a,
    const MAX_FRAMES: usize,
    const MAX_PDU_DATA: usize,
    const MAX_SLAVES: usize,
    TIMEOUT,
> {
    client: &'a Client<MAX_FRAMES, MAX_PDU_DATA, MAX_SLAVES, TIMEOUT>,
    configured_address: u16,
}

impl<'a, const MAX_FRAMES: usize, const MAX_PDU_DATA: usize, const MAX_SLAVES: usize, TIMEOUT>
    Eeprom<'a, MAX_FRAMES, MAX_PDU_DATA, MAX_SLAVES, TIMEOUT>
where
    TIMEOUT: TimerFactory,
{
    pub(crate) fn new(
        configured_address: u16,
        client: &'a Client<MAX_FRAMES, MAX_PDU_DATA, MAX_SLAVES, TIMEOUT>,
    ) -> Self {
        Self {
            client,
            configured_address,
        }
    }

    // TODO: Make a new SiiRead trait instead of repurposing PduRead - some types can only be read
    // from EEPROM.
    // TODO: EEPROM-specific error type
    pub async fn read_eeprom<T>(&self, eeprom_address: SiiCoding) -> Result<T, Error>
    where
        T: PduRead,
    {
        // TODO: Make this a const condition when possible
        debug_assert!(T::LEN <= 8);

        let eeprom_address = u16::from(eeprom_address);

        let buf = self.read_eeprom_raw(eeprom_address).await?;

        let buf = buf
            .as_slice()
            .get(0..usize::from(T::LEN))
            .ok_or(Error::EepromDecode)?;

        T::try_from_slice(buf).map_err(|_| Error::EepromDecode)
    }

    // TODO: Un-pub
    pub async fn read_eeprom_raw(
        &self,
        eeprom_address: impl Into<u16>,
    ) -> Result<EepromRead, Error> {
        let eeprom_address: u16 = eeprom_address.into();

        // TODO: Check EEPROM error flags

        let setup = SiiRequest::read(eeprom_address);

        // Set up an SII read. This writes the control word and the register word after it
        self.client
            .fpwr(
                self.configured_address,
                RegisterAddress::SiiControl,
                setup.to_array(),
            )
            .await?
            .wkc(1, "SII read setup")?;

        // TODO: Configurable timeout
        let timeout = core::time::Duration::from_millis(10);

        let read_size = crate::timeout::<TIMEOUT, _, _>(timeout, async {
            loop {
                let control = self
                    .client
                    .fprd::<SiiControl>(self.configured_address, RegisterAddress::SiiControl)
                    .await?
                    .wkc(1, "SII busy wait")?;

                if control.busy == false {
                    break Ok(control.read_size);
                }

                // TODO: Configurable loop tick
                TIMEOUT::timer(core::time::Duration::from_millis(1)).await;
            }
        })
        .await?;

        let data = match read_size {
            SiiReadSize::Octets4 => {
                let data = self
                    .client
                    .fprd::<[u8; 4]>(self.configured_address, RegisterAddress::SiiData)
                    .await?
                    .wkc(1, "SII data")?;

                log::trace!("Read {:#04x?} {:02x?}", eeprom_address, data);

                EepromRead::from(data)
            }
            SiiReadSize::Octets8 => {
                let data = self
                    .client
                    .fprd::<[u8; 8]>(self.configured_address, RegisterAddress::SiiData)
                    .await?
                    .wkc(1, "SII data")?;

                log::trace!("Read {:#04x?} {:02x?}", eeprom_address, data);

                EepromRead::from(data)
            }
        };

        Ok(data)
    }

    pub async fn device_name<const N: usize>(&self) -> Result<Option<heapless::String<N>>, Error> {
        let general = self.general().await?;

        let name_idx = general.name_string_idx;

        self.find_string(name_idx).await
    }

    async fn general(&self) -> Result<SiiGeneral, Error> {
        let category = self
            .find_eeprom_category_start(CategoryType::General)
            .await?
            .ok_or(Error::EepromNoCategory)?;

        let mut reader = EepromSectionReader::new(self, category);

        let buf = reader
            .take_vec_exact::<{ mem::size_of::<SiiGeneral>() }>()
            .await?;

        let (_, general) = SiiGeneral::parse(&buf).expect("General parse");

        Ok(general)
    }

    pub async fn sync_managers(&self) -> Result<heapless::Vec<SyncManager, 8>, Error> {
        let category = self
            .find_eeprom_category_start(CategoryType::SyncManager)
            .await?;

        let mut sync_managers = heapless::Vec::<_, 8>::new();

        if let Some(category) = category {
            let mut reader = EepromSectionReader::new(self, category);

            while let Some(bytes) = reader
                .take_vec::<{ mem::size_of::<SyncManager>() }>()
                .await?
            {
                let (_, sm) = SyncManager::parse(&bytes).unwrap();

                sync_managers.push(sm).unwrap();
            }
        }

        Ok(sync_managers)
    }

    // TODO: Define FMMU config struct and load from FMMU and FMMU_EX

    async fn find_string<const N: usize>(
        &self,
        search_index: u8,
    ) -> Result<Option<heapless::String<N>>, Error> {
        // An index of zero in EtherCAT denotes an empty string.
        if search_index == 0 {
            return Ok(None);
        }

        // Turn 1-based EtherCAT string indexing into normal 0-based.
        let search_index = search_index - 1;

        let category = self
            .find_eeprom_category_start(CategoryType::Strings)
            .await?;

        if let Some(category) = category {
            let mut reader = EepromSectionReader::new(self, category);

            let num_strings = reader.try_next().await?;

            if search_index > num_strings {
                return Ok(None);
            }

            for _ in 0..search_index {
                let string_len = reader.try_next().await?;

                reader.skip(u16::from(string_len)).await?;
            }

            let string_len = reader.try_next().await?;

            let bytes = reader
                .take_n_vec_exact::<N>(usize::from(string_len))
                .await?;

            let s = core::str::from_utf8(&bytes).map_err(|_| Error::EepromDecode)?;

            let s = heapless::String::<N>::from_str(s).map_err(|_| Error::EepromDecode)?;

            Ok(Some(s))
        } else {
            Ok(None)
        }
    }

    async fn find_eeprom_category_start(
        &self,
        category: CategoryType,
    ) -> Result<Option<SiiCategory>, Error> {
        let mut start = SII_FIRST_SECTION_START;

        loop {
            // First address, returns 2 bytes, contains the category type.
            let (category_type, data_len) = self.read_eeprom_raw(start).await.map(|chunk| {
                let chunk = chunk.bytes4();

                // TODO: Use array_chunks or similar method when stabilised
                let category = u16::from_le_bytes([chunk[0], chunk[1]]);
                let len = u16::from_le_bytes([chunk[2], chunk[3]]);

                (
                    CategoryType::try_from(category).unwrap_or(CategoryType::Nop),
                    len,
                )
            })?;

            // Position after header
            start += 2;

            log::debug!(
                "Found category {:?}, data starts at {start:#06x?}, length {:#04x?} ({}) bytes",
                category_type,
                data_len,
                data_len
            );

            match category_type {
                cat if cat == category => {
                    break Ok(Some(SiiCategory {
                        category: cat,
                        start,
                        len_words: data_len,
                    }))
                }
                CategoryType::End => break Ok(None),
                _ => (),
            }

            // Next category starts after the current category's data
            start += data_len;
        }
    }
}
