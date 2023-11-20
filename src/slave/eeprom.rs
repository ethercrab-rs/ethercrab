use num_enum::TryFromPrimitive;

use super::SlaveRef;
use crate::{
    eeprom::types::{FmmuEx, FmmuUsage, Pdo, SyncManager},
    eeprom::{
        reader::EepromSectionReader,
        types::{
            CategoryType, DefaultMailbox, FromEeprom, PdoEntry, SiiGeneral, RX_PDO_RANGE,
            TX_PDO_RANGE,
        },
    },
    error::{EepromError, Error, Item},
    fmt,
    slave::SlaveIdentity,
};
use core::{ops::RangeInclusive, str::FromStr};

/// EEPROM methods.
impl<'a, S> SlaveRef<'a, S> {
    /// Get the device name.
    ///
    /// Note that the string index is hard coded to `1` instead of reading the string index from the
    /// EEPROM `General` section.
    pub(crate) async fn eeprom_device_name<const N: usize>(
        &self,
    ) -> Result<Option<heapless::String<N>>, Error> {
        // Uncomment to read longer, but correct, name string from EEPROM
        // let general = self.general().await?;
        // let name_idx = general.name_string_idx;

        fmt::trace!("Get device name");

        // NOTE: Hard coded to the first string. This mirrors SOEM's behaviour. Reading the
        // string index from EEPROM gives a different value in my testing - still a name, but
        // longer.
        let name_idx = 1;

        self.eeprom_find_string(name_idx).await
    }

    pub(crate) async fn eeprom_mailbox_config(&self) -> Result<DefaultMailbox, Error> {
        // Start reading standard mailbox config. Raw start address defined in ETG2010 Table 2.
        // Mailbox config is 10 bytes long.
        let mut reader = EepromSectionReader::start_at(
            &self.client,
            0x0018,
            DefaultMailbox::STORAGE_SIZE as u16,
        );

        fmt::trace!("Get mailbox config");

        let buf = reader
            .take_vec_exact::<{ DefaultMailbox::STORAGE_SIZE }>()
            .await?;

        DefaultMailbox::parse(&buf)
    }

    pub(crate) async fn eeprom_general(&self) -> Result<SiiGeneral, Error> {
        let mut reader = EepromSectionReader::new(&self.client, CategoryType::General)
            .await?
            .ok_or(Error::Eeprom(EepromError::NoCategory))?;

        let buf = reader
            .take_vec_exact::<{ SiiGeneral::STORAGE_SIZE }>()
            .await?;

        SiiGeneral::parse(&buf)
    }

    pub(crate) async fn eeprom_identity(&self) -> Result<SlaveIdentity, Error> {
        let mut reader =
            EepromSectionReader::start_at(&self.client, 0x0008, SlaveIdentity::STORAGE_SIZE as u16);

        fmt::trace!("Get identity");

        reader
            .take_vec_exact::<{ SlaveIdentity::STORAGE_SIZE }>()
            .await
            .and_then(|buf| SlaveIdentity::parse(&buf))
    }

    pub(crate) async fn eeprom_sync_managers(
        &self,
    ) -> Result<heapless::Vec<SyncManager, 8>, Error> {
        let mut sync_managers = heapless::Vec::<_, 8>::new();

        fmt::trace!("Get sync managers");

        if let Some(mut reader) =
            EepromSectionReader::new(&self.client, CategoryType::SyncManager).await?
        {
            while let Some(bytes) = reader.take_vec::<{ SyncManager::STORAGE_SIZE }>().await? {
                let sm = SyncManager::parse(&bytes)?;

                sync_managers
                    .push(sm)
                    .map_err(|_| Error::Capacity(Item::SyncManager))?;
            }
        }

        fmt::debug!("Discovered sync managers:\n{:#?}", sync_managers);

        Ok(sync_managers)
    }

    pub(crate) async fn eeprom_fmmus(&self) -> Result<heapless::Vec<FmmuUsage, 16>, Error> {
        let category = EepromSectionReader::new(&self.client, CategoryType::Fmmu).await?;

        fmt::trace!("Get FMMUs");

        // ETG100.4 6.6.1 states there may be up to 16 FMMUs
        let mut fmmus = heapless::Vec::<_, 16>::new();

        if let Some(mut reader) = category {
            while let Some(byte) = reader.next().await? {
                let fmmu = FmmuUsage::try_from_primitive(byte)
                    .map_err(|_| Error::Eeprom(EepromError::Decode))?;

                fmmus.push(fmmu).map_err(|_| Error::Capacity(Item::Fmmu))?;
            }
        }

        fmt::debug!("Discovered FMMUs:\n{:#?}", fmmus);

        Ok(fmmus)
    }

    pub(crate) async fn eeprom_fmmu_mappings(&self) -> Result<heapless::Vec<FmmuEx, 16>, Error> {
        let mut mappings = heapless::Vec::<_, 16>::new();

        fmt::trace!("Get FMMU mappings");

        if let Some(mut reader) =
            EepromSectionReader::new(&self.client, CategoryType::FmmuExtended).await?
        {
            while let Some(bytes) = reader.take_vec::<{ FmmuEx::STORAGE_SIZE }>().await? {
                let fmmu = FmmuEx::parse(&bytes)?;

                mappings
                    .push(fmmu)
                    .map_err(|_| Error::Capacity(Item::FmmuEx))?;
            }
        }

        fmt::debug!("FMMU mappings: {:#?}", mappings);

        Ok(mappings)
    }

    async fn eeprom_pdos(
        &self,
        category: CategoryType,
        valid_range: RangeInclusive<u16>,
    ) -> Result<heapless::Vec<Pdo, 16>, Error> {
        let mut pdos = heapless::Vec::new();

        fmt::trace!("Get {:?} PDUs", category);

        if let Some(mut reader) = EepromSectionReader::new(&self.client, category).await? {
            while let Some(pdo) = reader.take_vec::<{ Pdo::STORAGE_SIZE }>().await? {
                let mut pdo = Pdo::parse(&pdo).map_err(|e| {
                    fmt::error!("PDO: {:?}", e);

                    Error::Eeprom(EepromError::Decode)
                })?;

                fmt::trace!("Range {:?} value {}", valid_range, pdo.index);

                if !valid_range.contains(&pdo.index) {
                    return Err(Error::Eeprom(EepromError::Decode));
                }

                for _ in 0..pdo.num_entries {
                    let entry = reader
                        .take_vec_exact::<{ PdoEntry::STORAGE_SIZE }>()
                        .await
                        .and_then(|bytes| {
                            let entry = PdoEntry::parse(&bytes).map_err(|e| {
                                fmt::error!("PDO entry: {:?}", e);

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

        fmt::debug!("Discovered PDOs:\n{:#?}", pdos);

        Ok(pdos)
    }

    /// Transmit PDOs (from device's perspective) - inputs
    pub(crate) async fn eeprom_master_read_pdos(&self) -> Result<heapless::Vec<Pdo, 16>, Error> {
        self.eeprom_pdos(CategoryType::TxPdo, TX_PDO_RANGE).await
    }

    /// Receive PDOs (from device's perspective) - outputs
    pub(crate) async fn eeprom_master_write_pdos(&self) -> Result<heapless::Vec<Pdo, 16>, Error> {
        self.eeprom_pdos(CategoryType::RxPdo, RX_PDO_RANGE).await
    }

    pub(crate) async fn eeprom_find_string<const N: usize>(
        &self,
        search_index: u8,
    ) -> Result<Option<heapless::String<N>>, Error> {
        fmt::trace!("Get string, index {}", search_index);

        // An index of zero in EtherCAT denotes an empty string.
        if search_index == 0 {
            return Ok(None);
        }

        // Turn 1-based EtherCAT string indexing into normal 0-based.
        let search_index = search_index - 1;

        if let Some(mut reader) =
            EepromSectionReader::new(&self.client, CategoryType::Strings).await?
        {
            let num_strings = reader.try_next().await?;

            fmt::trace!("--> Slave has {} strings", num_strings);

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

            fmt::trace!("--> Raw string bytes {:?}", bytes);

            let s = core::str::from_utf8(&bytes).map_err(|_| Error::Eeprom(EepromError::Decode))?;

            // Strip trailing null bytes from string.
            // TODO: Unit test this when an EEPROM shim is added
            let s = s.trim_end_matches('\0');

            let s = heapless::String::<N>::from_str(s)
                .map_err(|_| Error::Eeprom(EepromError::Decode))?;

            fmt::trace!(
                "--> String at search index {} with length {}: {}",
                search_index,
                string_len,
                s
            );

            Ok(Some(s))
        } else {
            Ok(None)
        }
    }
}
