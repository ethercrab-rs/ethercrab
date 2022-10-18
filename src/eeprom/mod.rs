mod reader;
pub mod types;

use self::types::{DefaultMailbox, FmmuEx};
use crate::{
    eeprom::{
        reader::EepromSectionReader,
        types::{
            CategoryType, FmmuUsage, Pdo, PdoEntry, SiiCategory, SiiControl, SiiGeneral,
            SiiReadSize, SiiRequest, SyncManager, RX_PDO_RANGE, TX_PDO_RANGE,
        },
    },
    error::{EepromError, Error, Item},
    register::RegisterAddress,
    slave::{slave_client::SlaveClient, SlaveIdentity},
    timer_factory::TimerFactory,
};
use core::{mem, ops::RangeInclusive, str::FromStr};
use num_enum::TryFromPrimitive;

/// The address of the first proper category, positioned after the fixed fields defined in ETG2010
/// Table 2.
const SII_FIRST_CATEGORY_START: u16 = 0x0040u16;

pub struct Eeprom<'a, const MAX_FRAMES: usize, const MAX_PDU_DATA: usize, TIMEOUT> {
    client: &'a SlaveClient<'a, MAX_FRAMES, MAX_PDU_DATA, TIMEOUT>,
}

impl<'a, const MAX_FRAMES: usize, const MAX_PDU_DATA: usize, TIMEOUT>
    Eeprom<'a, MAX_FRAMES, MAX_PDU_DATA, TIMEOUT>
where
    TIMEOUT: TimerFactory,
{
    pub(crate) fn new(client: &'a SlaveClient<'a, MAX_FRAMES, MAX_PDU_DATA, TIMEOUT>) -> Self {
        Self { client }
    }

    async fn read_eeprom_raw(&self, eeprom_address: u16) -> Result<[u8; 8], Error> {
        let status = self
            .client
            .read::<SiiControl>(RegisterAddress::SiiControl, "Read SII control")
            .await?;

        // Clear errors
        if status.has_error() {
            log::trace!("Resetting EEPROM error flags");

            self.client
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
        self.client
            .write(
                RegisterAddress::SiiControl,
                SiiRequest::read(eeprom_address).as_array(),
                "SII read setup",
            )
            .await?;

        self.wait().await?;

        let data = match status.read_size {
            // If slave uses 4 octet reads, do two reads so we can always return a chunk of 8 bytes
            SiiReadSize::Octets4 => {
                let chunk1 = self
                    .client
                    .read::<[u8; 4]>(RegisterAddress::SiiData, "Read SII data")
                    .await?;

                // Move on to next chunk
                {
                    // NOTE: We must compute offset in 16 bit words, not bytes, hence the divide by 2
                    let setup = SiiRequest::read(eeprom_address + (chunk1.len() / 2) as u16);

                    self.client
                        .write(
                            RegisterAddress::SiiControl,
                            setup.as_array(),
                            "SII read setup",
                        )
                        .await?;

                    self.wait().await?;
                }

                let chunk2 = self
                    .client
                    .read::<[u8; 4]>(RegisterAddress::SiiData, "SII data 2")
                    .await?;

                let mut data = [0u8; 8];

                data[0..4].copy_from_slice(&chunk1);
                data[4..8].copy_from_slice(&chunk2);

                data
            }
            SiiReadSize::Octets8 => {
                self.client
                    .read::<[u8; 8]>(RegisterAddress::SiiData, "SII data")
                    .await?
            }
        };

        log::trace!("Read {:#04x?} {:02x?}", eeprom_address, data);

        Ok(data)
    }

    /// Wait for EEPROM read or write operation to finish and clear the busy flag.
    async fn wait(&self) -> Result<(), Error> {
        crate::timer_factory::timeout::<TIMEOUT, _, _>(self.client.timeouts().eeprom, async {
            loop {
                let control = self
                    .client
                    .read::<SiiControl>(RegisterAddress::SiiControl, "SII busy wait")
                    .await?;

                if !control.busy {
                    break Ok(());
                }

                // TODO: Configurable loop tick
                TIMEOUT::timer(self.client.timeouts().wait_loop_delay).await;
            }
        })
        .await
    }

    /// Get the device name.
    ///
    /// Note that the string index is hard coded to `1` instead of reading the string index from the
    /// EEPROM `General` section.
    pub async fn device_name<const N: usize>(&self) -> Result<Option<heapless::String<N>>, Error> {
        // Uncomment to read longer, but correct, name string from EEPROM
        // let general = self.general().await?;
        // let name_idx = general.name_string_idx;

        // NOTE: Hard coded to the first string. This mirrors SOEM's behaviour. Reading the
        // string index from EEPROM gives a different value in my testing - still a name, but
        // longer.
        let name_idx = 1;

        self.find_string(name_idx).await
    }

    pub async fn mailbox_config(&self) -> Result<DefaultMailbox, Error> {
        // Start reading standard mailbox config. Raw start address defined in ETG2010 Table 2.
        // Mailbox config is 10 bytes long.
        let mut reader = EepromSectionReader::start_at(self, 0x0018, 10);

        let buf = reader.take_vec_exact::<10>().await?;

        let (_, config) = DefaultMailbox::parse(&buf).expect("General parse");

        Ok(config)
    }

    pub(crate) async fn general(&self) -> Result<SiiGeneral, Error> {
        let mut reader = self
            .find_category(CategoryType::General)
            .await?
            .ok_or(Error::Eeprom(EepromError::NoCategory))?;

        let buf = reader
            .take_vec_exact::<{ mem::size_of::<SiiGeneral>() }>()
            .await?;

        let (_, general) = SiiGeneral::parse(&buf).expect("General parse");

        Ok(general)
    }

    pub async fn identity(&self) -> Result<SlaveIdentity, Error> {
        let mut reader = EepromSectionReader::start_at(self, 0x0008, 16);

        let buf = reader
            .take_vec_exact::<{ mem::size_of::<SlaveIdentity>() }>()
            .await?;

        let (_, general) = SlaveIdentity::parse(&buf).expect("Slave identity parse");

        Ok(general)
    }

    pub async fn sync_managers(&self) -> Result<heapless::Vec<SyncManager, 8>, Error> {
        let mut sync_managers = heapless::Vec::<_, 8>::new();

        if let Some(mut reader) = self.find_category(CategoryType::SyncManager).await? {
            while let Some(bytes) = reader.take_vec::<8>().await? {
                let (_, sm) = SyncManager::parse(&bytes).unwrap();

                sync_managers
                    .push(sm)
                    .map_err(|_| Error::Capacity(Item::SyncManager))?;
            }
        }

        Ok(sync_managers)
    }

    pub async fn fmmus(&self) -> Result<heapless::Vec<FmmuUsage, 16>, Error> {
        let category = self.find_category(CategoryType::Fmmu).await?;

        // ETG100.4 6.6.1 states there may be up to 16 FMMUs
        let mut fmmus = heapless::Vec::<_, 16>::new();

        if let Some(mut reader) = category {
            while let Some(byte) = reader.next().await? {
                let fmmu = FmmuUsage::try_from_primitive(byte)
                    .map_err(|_| Error::Eeprom(EepromError::Decode))?;

                fmmus.push(fmmu).map_err(|_| Error::Capacity(Item::Fmmu))?;
            }
        }

        Ok(fmmus)
    }

    pub async fn fmmu_mappings(&self) -> Result<heapless::Vec<FmmuEx, 16>, Error> {
        let mut mappings = heapless::Vec::<_, 16>::new();

        if let Some(mut reader) = self.find_category(CategoryType::FmmuExtended).await? {
            while let Some(bytes) = reader.take_vec::<3>().await? {
                let (_, fmmu) = FmmuEx::parse(&bytes).unwrap();

                mappings
                    .push(fmmu)
                    .map_err(|_| Error::Capacity(Item::FmmuEx))?;
            }
        }

        Ok(mappings)
    }

    async fn pdos(
        &self,
        category: CategoryType,
        valid_range: RangeInclusive<u16>,
    ) -> Result<heapless::Vec<Pdo, 16>, Error> {
        let mut pdos = heapless::Vec::new();

        if let Some(mut reader) = self.find_category(category).await? {
            // TODO: Define a trait that gives the number of bytes to take to parse the type.
            while let Some(pdo) = reader.take_n_vec::<8>(8).await? {
                let (_, mut pdo) = Pdo::parse(&pdo).map_err(|e| {
                    log::error!("PDO: {}", e);

                    Error::Eeprom(EepromError::Decode)
                })?;

                log::trace!("Range {:?} value {}", valid_range, pdo.index);

                if !valid_range.contains(&pdo.index) {
                    return Err(Error::Eeprom(EepromError::Decode));
                }

                for _ in 0..pdo.num_entries {
                    let entry = reader.take_n_vec_exact::<8>(8).await.and_then(|bytes| {
                        let (_, entry) = PdoEntry::parse(&bytes).map_err(|e| {
                            log::error!("PDO entry: {}", e);

                            Error::Eeprom(EepromError::Decode)
                        })?;

                        Ok(entry)
                    })?;

                    pdo.entries
                        .push(entry)
                        .map_err(|_| Error::Capacity(Item::PdoEntry))?;
                }

                pdos.push(pdo).map_err(|_| Error::Capacity(Item::Pdo))?;
            }
        }

        Ok(pdos)
    }

    /// Transmit PDOs (from device's perspective) - inputs
    pub async fn master_read_pdos(&self) -> Result<heapless::Vec<Pdo, 16>, Error> {
        self.pdos(CategoryType::TxPdo, TX_PDO_RANGE).await
    }

    /// Receive PDOs (from device's perspective) - outputs
    pub async fn master_write_pdos(&self) -> Result<heapless::Vec<Pdo, 16>, Error> {
        self.pdos(CategoryType::RxPdo, RX_PDO_RANGE).await
    }

    pub(crate) async fn find_string<const N: usize>(
        &self,
        search_index: u8,
    ) -> Result<Option<heapless::String<N>>, Error> {
        // An index of zero in EtherCAT denotes an empty string.
        if search_index == 0 {
            return Ok(None);
        }

        // Turn 1-based EtherCAT string indexing into normal 0-based.
        let search_index = search_index - 1;

        if let Some(mut reader) = self.find_category(CategoryType::Strings).await? {
            let num_strings = reader.try_next().await?;

            if search_index > num_strings {
                return Ok(None);
            }

            for _ in 0..search_index {
                let string_len = reader.try_next().await?;

                reader.skip(u16::from(string_len)).await?;
            }

            let string_len = usize::from(reader.try_next().await?);

            let bytes = reader
                .take_n_vec_exact::<N>(string_len)
                .await
                .map_err(|_| Error::StringTooLong {
                    desired: N,
                    required: string_len,
                })?;

            let s = core::str::from_utf8(&bytes).map_err(|_| Error::Eeprom(EepromError::Decode))?;

            let s = heapless::String::<N>::from_str(s)
                .map_err(|_| Error::Eeprom(EepromError::Decode))?;

            Ok(Some(s))
        } else {
            Ok(None)
        }
    }

    async fn find_category(
        &self,
        category: CategoryType,
    ) -> Result<Option<EepromSectionReader<'_, MAX_FRAMES, MAX_PDU_DATA, TIMEOUT>>, Error> {
        let mut start = SII_FIRST_CATEGORY_START;

        loop {
            let chunk = self.read_eeprom_raw(start).await?;

            let category_type =
                CategoryType::from(u16::from_le_bytes(chunk[0..2].try_into().unwrap()));
            let data_len = u16::from_le_bytes(chunk[2..4].try_into().unwrap());

            // Position after header
            start += 2;

            log::trace!(
                "Found category {category_type:?}, data starts at {start:#06x?}, length {:#04x?} ({}) bytes",
                data_len,
                data_len
            );

            match category_type {
                cat if cat == category => {
                    break Ok(Some(EepromSectionReader::new(
                        self,
                        SiiCategory {
                            category: cat,
                            start,
                            len_words: data_len,
                        },
                    )))
                }
                CategoryType::End => break Ok(None),
                _ => (),
            }

            // Next category starts after the current category's data
            start += data_len;
        }
    }
}
