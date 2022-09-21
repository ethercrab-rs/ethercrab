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

        let len = usize::from(category.len);
        let mut start = category.start;

        // Chunks are read in multiples of 4 or 8 and we need at least 18 bytes
        let mut buf = heapless::Vec::<u8, 24>::new();

        // TODO: This loop needs splitting into a function which fills up a slice and returns it
        let buf = loop {
            let sl = self.read_eeprom_raw(start).await.unwrap();
            // Each EEPROM address contains 2 bytes, so we need to step half as fast
            start += sl.as_slice().len() as u16 / 2;

            buf.extend_from_slice(sl.as_slice())
                .expect("Buffer is full");

            if buf.len() >= len {
                break &buf[0..len];
            }
        };

        let (_, general) = SiiGeneral::parse(buf).expect("General parse");

        Ok(general)
    }

    pub async fn sync_managers(&self) -> Result<heapless::Vec<SyncManager, 8>, Error> {
        let category = self
            .find_eeprom_category_start(CategoryType::SyncManager)
            .await?;

        let mut sync_managers = heapless::Vec::<_, 8>::new();

        if let Some(category) = category {
            let len = usize::from(category.len);
            let mut start = category.start;
            let end = start + len as u16;

            while start <= end {
                let mut buf = heapless::Vec::<u8, 8>::new();

                while buf.len() < 8 {
                    let sl = self.read_eeprom_raw(start).await?;
                    start += dbg!(sl.as_slice()).len() as u16;

                    buf.extend_from_slice(sl.as_slice()).unwrap();
                }

                let (_, sm) = SyncManager::parse(&buf).unwrap();

                sync_managers.push(sm).unwrap();
            }
        }

        Ok(sync_managers)
    }

    // TODO: Define FMMU config struct and load from FMMU and FMMU_EX

    /// Find a string by index.
    ///
    /// Note that SII string indices start at 1, not 0.
    ///
    /// Passing an index of 0 will return `None`, although the spec defines index 0 as an empty
    /// string as per ETG1000.6 Table 20 footnote.
    // TODO: This is a pretty inefficient algorithm. Find a way to use only `N` bytes as well as
    // skipping ignored strings instead of reading them into the buffer.
    // FIXME: Shitload of unwraps/panics/expects
    async fn find_string<const N: usize>(
        &self,
        search_index: u8,
    ) -> Result<Option<heapless::String<N>>, Error> {
        if search_index == 0 {
            return Ok(None);
        }

        let pos = self
            .find_eeprom_category_start(CategoryType::Strings)
            .await?;

        if let Some(pos) = pos {
            let mut start = pos.start;

            let read = self.read_eeprom_raw(start).await.unwrap();

            let sl = read.as_slice();

            // Each EEPROM address contains 2 bytes, so we need to step half as fast
            start += sl.len() as u16 / 2;

            // The first byte of the strings section is the number of strings contained within it
            let (num_strings, buf) = sl.split_first().expect("Split first");
            let num_strings = *num_strings;

            log::debug!("Found {num_strings} strings");

            // Initialise the buffer with the remaining first read
            // TODO: Use `{ N + 8 }` when generic_const_exprs is stabilised
            let mut buf = heapless::Vec::<u8, 255>::from_slice(buf).unwrap();

            for idx in 0..num_strings {
                // TODO: DRY: This loop needs splitting into a function which fills up a slice and returns it
                loop {
                    let read = self.read_eeprom_raw(start).await.unwrap();
                    let sl = read.as_slice();

                    // Each EEPROM address contains 2 bytes, so we need to step half as fast
                    start += sl.len() as u16 / 2;
                    buf.extend_from_slice(sl).expect("Buffer is full");

                    let i = buf.as_slice();

                    let i = match length_data::<_, _, (), _>(le_u8)(i) {
                        Ok((i, string_data)) => {
                            if idx == search_index.saturating_sub(1) {
                                let s = core::str::from_utf8(string_data)
                                    .map_err(|_| Error::EepromDecode)?;

                                let s = heapless::String::from_str(s)
                                    .map_err(|_| Error::EepromDecode)?;

                                return Ok(Some(s));
                            }

                            i
                        }
                        Err(e) => match e {
                            nom::Err::Incomplete(_needed) => {
                                continue;
                            }
                            nom::Err::Error(e) => panic!("Error {e:?}"),
                            nom::Err::Failure(e) => panic!("Fail {e:?}"),
                        },
                    };

                    buf = heapless::Vec::from_slice(i).unwrap();

                    break;
                }
            }

            // TODO: Index out of bounds error
            Err(Error::EepromDecode)
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
                        len: data_len,
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
