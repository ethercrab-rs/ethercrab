mod reader;
pub mod types;

use crate::{
    eeprom::{
        reader::EepromSectionReader,
        types::{
            CategoryType, DefaultMailbox, FmmuEx, FmmuUsage, FromEeprom, Pdo, PdoEntry, SiiGeneral,
            SyncManager, RX_PDO_RANGE, TX_PDO_RANGE,
        },
    },
    error::{EepromError, Error, Item},
    slave::{slave_client::SlaveClient, SlaveIdentity},
};
use core::{ops::RangeInclusive, str::FromStr};
use num_enum::TryFromPrimitive;

#[derive(Debug)]
pub struct Eeprom<'a> {
    client: &'a SlaveClient<'a>,
}

impl<'a> Eeprom<'a> {
    pub(crate) fn new(client: &'a SlaveClient<'a>) -> Self {
        Self { client }
    }

    async fn reader(
        &self,
        category: CategoryType,
    ) -> Result<Option<EepromSectionReader<'_>>, Error> {
        EepromSectionReader::new(self.client, category).await
    }

    /// Get the device name.
    ///
    /// Note that the string index is hard coded to `1` instead of reading the string index from the
    /// EEPROM `General` section.
    pub async fn device_name<const N: usize>(&self) -> Result<Option<heapless::String<N>>, Error> {
        // Uncomment to read longer, but correct, name string from EEPROM
        // let general = self.general().await?;
        // let name_idx = general.name_string_idx;

        log::trace!("Get device name");

        // NOTE: Hard coded to the first string. This mirrors SOEM's behaviour. Reading the
        // string index from EEPROM gives a different value in my testing - still a name, but
        // longer.
        let name_idx = 1;

        self.find_string(name_idx).await
    }

    pub async fn mailbox_config(&self) -> Result<DefaultMailbox, Error> {
        // Start reading standard mailbox config. Raw start address defined in ETG2010 Table 2.
        // Mailbox config is 10 bytes long.
        let mut reader =
            EepromSectionReader::start_at(self.client, 0x0018, DefaultMailbox::STORAGE_SIZE as u16);

        log::trace!("Get mailbox config");

        let buf = reader
            .take_vec_exact::<{ DefaultMailbox::STORAGE_SIZE }>()
            .await?;

        DefaultMailbox::parse(&buf)
    }

    #[allow(unused)]
    pub(crate) async fn general(&self) -> Result<SiiGeneral, Error> {
        let mut reader = self
            .reader(CategoryType::General)
            .await?
            .ok_or(Error::Eeprom(EepromError::NoCategory))?;

        let buf = reader
            .take_vec_exact::<{ SiiGeneral::STORAGE_SIZE }>()
            .await?;

        SiiGeneral::parse(&buf)
    }

    pub async fn identity(&self) -> Result<SlaveIdentity, Error> {
        let mut reader =
            EepromSectionReader::start_at(self.client, 0x0008, SlaveIdentity::STORAGE_SIZE as u16);

        log::trace!("Get identity");

        reader
            .take_vec_exact::<{ SlaveIdentity::STORAGE_SIZE }>()
            .await
            .and_then(|buf| SlaveIdentity::parse(&buf))
    }

    pub async fn sync_managers(&self) -> Result<heapless::Vec<SyncManager, 8>, Error> {
        let mut sync_managers = heapless::Vec::<_, 8>::new();

        log::trace!("Get sync managers");

        if let Some(mut reader) = self.reader(CategoryType::SyncManager).await? {
            while let Some(bytes) = reader.take_vec::<{ SyncManager::STORAGE_SIZE }>().await? {
                let sm = SyncManager::parse(&bytes)?;

                sync_managers
                    .push(sm)
                    .map_err(|_| Error::Capacity(Item::SyncManager))?;
            }
        }

        log::debug!("Discovered sync managers:\n{:#?}", sync_managers);

        Ok(sync_managers)
    }

    pub async fn fmmus(&self) -> Result<heapless::Vec<FmmuUsage, 16>, Error> {
        let category = self.reader(CategoryType::Fmmu).await?;

        log::trace!("Get FMMUs");

        // ETG100.4 6.6.1 states there may be up to 16 FMMUs
        let mut fmmus = heapless::Vec::<_, 16>::new();

        if let Some(mut reader) = category {
            while let Some(byte) = reader.next().await? {
                let fmmu = FmmuUsage::try_from_primitive(byte)
                    .map_err(|_| Error::Eeprom(EepromError::Decode))?;

                fmmus.push(fmmu).map_err(|_| Error::Capacity(Item::Fmmu))?;
            }
        }

        log::debug!("Discovered FMMUs:\n{:#?}", fmmus);

        Ok(fmmus)
    }

    pub async fn fmmu_mappings(&self) -> Result<heapless::Vec<FmmuEx, 16>, Error> {
        let mut mappings = heapless::Vec::<_, 16>::new();

        log::trace!("Get FMMU mappings");

        if let Some(mut reader) = self.reader(CategoryType::FmmuExtended).await? {
            while let Some(bytes) = reader.take_vec::<{ FmmuEx::STORAGE_SIZE }>().await? {
                let fmmu = FmmuEx::parse(&bytes)?;

                mappings
                    .push(fmmu)
                    .map_err(|_| Error::Capacity(Item::FmmuEx))?;
            }
        }

        log::debug!("FMMU mappings: {:#?}", mappings);

        Ok(mappings)
    }

    async fn pdos(
        &self,
        category: CategoryType,
        valid_range: RangeInclusive<u16>,
    ) -> Result<heapless::Vec<Pdo, 16>, Error> {
        let mut pdos = heapless::Vec::new();

        log::trace!("Get {:?} PDUs", category);

        if let Some(mut reader) = self.reader(category).await? {
            while let Some(pdo) = reader.take_vec::<{ Pdo::STORAGE_SIZE }>().await? {
                let mut pdo = Pdo::parse(&pdo).map_err(|e| {
                    log::error!("PDO: {:?}", e);

                    Error::Eeprom(EepromError::Decode)
                })?;

                log::trace!("Range {:?} value {}", valid_range, pdo.index);

                if !valid_range.contains(&pdo.index) {
                    return Err(Error::Eeprom(EepromError::Decode));
                }

                for _ in 0..pdo.num_entries {
                    let entry = reader
                        .take_vec_exact::<{ PdoEntry::STORAGE_SIZE }>()
                        .await
                        .and_then(|bytes| {
                            let entry = PdoEntry::parse(&bytes).map_err(|e| {
                                log::error!("PDO entry: {:?}", e);

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

        log::debug!("Discovered PDOs:\n{:#?}", pdos);

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
        log::trace!("Get string, index {}", search_index);

        // An index of zero in EtherCAT denotes an empty string.
        if search_index == 0 {
            return Ok(None);
        }

        // Turn 1-based EtherCAT string indexing into normal 0-based.
        let search_index = search_index - 1;

        if let Some(mut reader) = self.reader(CategoryType::Strings).await? {
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
                .take_vec_len_exact::<N>(string_len)
                .await
                .map_err(|_| Error::StringTooLong {
                    max_length: N,
                    string_length: string_len,
                })?;

            let s = core::str::from_utf8(&bytes).map_err(|_| Error::Eeprom(EepromError::Decode))?;

            let s = heapless::String::<N>::from_str(s)
                .map_err(|_| Error::Eeprom(EepromError::Decode))?;

            Ok(Some(s))
        } else {
            Ok(None)
        }
    }
}
