use embedded_io_async::{ErrorType, Read, Seek, SeekFrom};
use num_enum::TryFromPrimitive;

use crate::{
    eeprom::types::{
        CategoryType, DefaultMailbox, FromEeprom, PdoEntry, SiiGeneral, RX_PDO_RANGE, TX_PDO_RANGE,
    },
    eeprom::{
        reader::SII_FIRST_CATEGORY_START,
        types::{FmmuEx, FmmuUsage, Pdo, SyncManager},
        ChunkReader, EepromDataProvider,
    },
    error::{EepromError, Error, Item},
    fmt,
    slave::SlaveIdentity,
};
use core::{ops::RangeInclusive, str::FromStr};

pub struct SlaveEeprom<P> {
    provider: P,
}

/// EEPROM methods.
impl<P> SlaveEeprom<P>
where
    P: EepromDataProvider,
{
    pub(crate) fn new(provider: P) -> Self {
        Self { provider }
    }

    async fn start_at(&self, addr: u16, len: u16) -> Result<ChunkReader<P::Provider>, Error> {
        let mut r = self.provider.reader();

        r.seek(SeekFrom::Start(addr.into())).await?;

        Ok(ChunkReader::new(r, len))
    }

    // Category search logic is moved here to reduce duplication in each impl.
    async fn category(
        &self,
        category: CategoryType,
    ) -> Result<Option<ChunkReader<P::Provider>>, Error> {
        let mut reader = self.provider.reader();

        reader
            .seek(SeekFrom::Start(SII_FIRST_CATEGORY_START.into()))
            .await?;

        loop {
            let mut header = [0u8; 4];

            reader.read_exact(&mut header).await?;

            // The chunk is either 4 or 8 bytes long, so these unwraps should never fire.
            let category_type =
                CategoryType::from(u16::from_le_bytes(fmt::unwrap!(header[0..2].try_into())));
            let len_words = u16::from_le_bytes(fmt::unwrap!(header[2..4].try_into()));

            // Position after header
            // Done inside read_exact
            // start_word += 2;

            fmt::trace!(
                "Found category {:?}, length {:#04x} ({}) bytes",
                category_type,
                len_words,
                len_words
            );

            match category_type {
                cat if cat == category => {
                    // break Ok(Some(reader.set_len(len_words * 2)));
                    break Ok(Some(ChunkReader::new(reader, len_words * 2)));
                }
                CategoryType::End => break Ok(None),
                _ => (),
            }

            // Next category starts after the current category's data. Seek takes a WORD address
            reader.seek(SeekFrom::Current(len_words.into())).await?;
        }
    }

    /// Get the device name.
    ///
    /// Note that the string index is hard coded to `1` instead of reading the string index from the
    /// EEPROM `General` section.
    pub(crate) async fn device_name<const N: usize>(
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

        self.find_string(name_idx).await
    }

    pub(crate) async fn mailbox_config(&self) -> Result<DefaultMailbox, Error> {
        // Start reading standard mailbox config. Raw start address defined in ETG2010 Table 2.
        // Mailbox config is 10 bytes long.
        let mut reader = self
            .start_at(0x0018, DefaultMailbox::STORAGE_SIZE as u16)
            .await?;

        fmt::trace!("Get mailbox config");

        let buf = reader
            .take_vec_exact::<{ DefaultMailbox::STORAGE_SIZE }>()
            .await?;

        DefaultMailbox::parse(&buf)
    }

    pub(crate) async fn general(&self) -> Result<SiiGeneral, Error> {
        let mut reader = self
            .category(CategoryType::General)
            .await?
            .ok_or(Error::Eeprom(EepromError::NoCategory))?;

        let buf = reader
            .take_vec_exact::<{ SiiGeneral::STORAGE_SIZE }>()
            .await?;

        SiiGeneral::parse(&buf)
    }

    pub(crate) async fn identity(&self) -> Result<SlaveIdentity, Error> {
        let mut reader = self
            .start_at(0x0008, SlaveIdentity::STORAGE_SIZE as u16)
            .await?;

        fmt::trace!("Get identity");

        reader
            .take_vec_exact::<{ SlaveIdentity::STORAGE_SIZE }>()
            .await
            .and_then(|buf| SlaveIdentity::parse(&buf))
    }

    pub(crate) async fn sync_managers(&self) -> Result<heapless::Vec<SyncManager, 8>, Error> {
        let mut sync_managers = heapless::Vec::<_, 8>::new();

        fmt::trace!("Get sync managers");

        if let Some(mut reader) = self.category(CategoryType::SyncManager).await? {
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

    pub(crate) async fn fmmus(&self) -> Result<heapless::Vec<FmmuUsage, 16>, Error> {
        let category = self.category(CategoryType::Fmmu).await?;

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

    pub(crate) async fn fmmu_mappings(&self) -> Result<heapless::Vec<FmmuEx, 16>, Error> {
        let mut mappings = heapless::Vec::<_, 16>::new();

        fmt::trace!("Get FMMU mappings");

        if let Some(mut reader) = self.category(CategoryType::FmmuExtended).await? {
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

    async fn pdos(
        &self,
        category: CategoryType,
        valid_range: RangeInclusive<u16>,
    ) -> Result<heapless::Vec<Pdo, 16>, Error> {
        let mut pdos = heapless::Vec::new();

        fmt::trace!("Get {:?} PDUs", category);

        if let Some(mut reader) = self.category(category).await? {
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
    pub(crate) async fn master_read_pdos(&self) -> Result<heapless::Vec<Pdo, 16>, Error> {
        self.pdos(CategoryType::TxPdo, TX_PDO_RANGE).await
    }

    /// Receive PDOs (from device's perspective) - outputs
    pub(crate) async fn master_write_pdos(&self) -> Result<heapless::Vec<Pdo, 16>, Error> {
        self.pdos(CategoryType::RxPdo, RX_PDO_RANGE).await
    }

    pub(crate) async fn find_string<const N: usize>(
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

        if let Some(mut reader) = self.category(CategoryType::Strings).await? {
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
