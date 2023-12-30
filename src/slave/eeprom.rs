use crate::{
    eeprom::types::{
        CategoryType, DefaultMailbox, FromEeprom, PdoEntry, SiiGeneral, RX_PDO_RANGE, TX_PDO_RANGE,
    },
    eeprom::{
        device_reader::SII_FIRST_CATEGORY_START,
        types::{FmmuEx, FmmuUsage, Pdo, PdoType, SyncManager},
        ChunkReader, EepromDataProvider,
    },
    error::{EepromError, Error, Item},
    fmt,
    slave::SlaveIdentity,
};
use core::{ops::RangeInclusive, str::FromStr};
use embedded_io_async::Read;
use num_enum::TryFromPrimitive;

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

    /// Start a reader at the given address in words, returning at most `len` bytes.
    async fn start_at(&self, word_addr: u16, len_bytes: u16) -> Result<ChunkReader<P>, Error> {
        // let mut r = self.provider.reader();

        // r.seek_to_word(SeekFrom::Start(word_addr.into())).await?;

        println!(
            "Word addr {} {:#04x}, len bytes {} ({} w)",
            word_addr,
            word_addr,
            len_bytes,
            len_bytes / 2
        );

        Ok(ChunkReader::new(
            self.provider.clone(),
            word_addr,
            len_bytes / 2,
        ))
    }

    /// Search for a given category and return a reader over the bytes contained within the category
    /// if it is found.
    async fn category(&self, category: CategoryType) -> Result<Option<ChunkReader<P>>, Error> {
        // let mut reader = ChunkReader::new(self.provider.clone(), 0, u16::MAX);

        let mut reader = self.provider.clone();

        // reader
        //     .seek_to_word(SeekFrom::Start(SII_FIRST_CATEGORY_START.into()))
        //     .await?;

        let mut word_addr = SII_FIRST_CATEGORY_START;

        loop {
            // let mut category_type = [0u8; 2];
            // let mut len_words = [0u8; 2];

            let chunk = reader.read_chunk(word_addr).await?;

            // category_type.copy_from_slice(&chunk[0..2]);
            // len_words.copy_from_slice(&chunk[0..2]);

            // reader.read_exact(&mut category_type).await?;
            // reader.read_exact(&mut len_words).await?;

            word_addr += 2;

            let category_type =
                CategoryType::from(u16::from_le_bytes(fmt::unwrap!(chunk[0..2].try_into())));
            let len_words = u16::from_le_bytes(fmt::unwrap!(chunk[2..4].try_into()));

            fmt::trace!(
                "Found category {:?} at {:#06x} bytes, length {:#04x} ({}) words",
                category_type,
                word_addr * 2,
                len_words,
                len_words
            );

            match category_type {
                cat if cat == category => {
                    break Ok(Some(ChunkReader::new(
                        self.provider.clone(),
                        word_addr,
                        len_words,
                    )));
                }
                CategoryType::End => break Ok(None),
                _ => (),
            }

            // Next category starts after the current category's data. This is a WORD address.
            word_addr += len_words;

            // reader
            //     .seek_to_word(SeekFrom::Current(len_words.into()))
            //     .await?;
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

        let mut buf = [0u8; { DefaultMailbox::STORAGE_SIZE }];

        reader.read_exact(&mut buf).await?;

        DefaultMailbox::parse(&buf)
    }

    pub(crate) async fn general(&self) -> Result<SiiGeneral, Error> {
        let mut reader = self
            .category(CategoryType::General)
            .await?
            .ok_or(Error::Eeprom(EepromError::NoCategory))?;

        let mut buf = [0u8; { SiiGeneral::STORAGE_SIZE }];

        reader.read_exact(&mut buf).await?;

        SiiGeneral::parse(&buf)
    }

    pub(crate) async fn identity(&self) -> Result<SlaveIdentity, Error> {
        let mut reader = self
            .start_at(0x0008, SlaveIdentity::STORAGE_SIZE as u16)
            .await?;

        fmt::trace!("Get identity");

        let mut buf = [0u8; { SlaveIdentity::STORAGE_SIZE }];

        reader.read_exact(&mut buf).await?;

        SlaveIdentity::parse(&buf)
    }

    pub(crate) async fn sync_managers(&self) -> Result<heapless::Vec<SyncManager, 8>, Error> {
        let mut sync_managers = heapless::Vec::<_, 8>::new();

        fmt::trace!("Get sync managers");

        if let Some(mut reader) = self.category(CategoryType::SyncManager).await? {
            let mut buf = [0u8; { SyncManager::STORAGE_SIZE }];

            while reader.read(&mut buf).await? == SyncManager::STORAGE_SIZE {
                let sm = SyncManager::parse(&buf)?;

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

        let fmmus = if let Some(mut reader) = category {
            // ETG100.4 6.6.1 states there may be up to 16 FMMUs
            let mut buf = [0u8; 16];

            // Read entire category using its discovered length.
            let fmmus = reader.read(&mut buf).await?;

            buf[0..fmmus]
                .into_iter()
                .map(|raw| {
                    FmmuUsage::try_from_primitive(*raw).map_err(|_e| {
                        #[cfg(feature = "std")]
                        fmt::error!("Failed to decode FmmuUsage: {}", _e);

                        Error::Eeprom(EepromError::Decode)
                    })
                })
                .collect::<Result<heapless::Vec<_, 16>, Error>>()?
        } else {
            // Category was not found so no FMMUs are present.
            heapless::Vec::<_, 16>::new()
        };

        fmt::debug!("Discovered FMMUs:\n{:#?}", fmmus);

        Ok(fmmus)
    }

    pub(crate) async fn fmmu_mappings(&self) -> Result<heapless::Vec<FmmuEx, 16>, Error> {
        let mut mappings = heapless::Vec::<_, 16>::new();

        fmt::trace!("Get FMMU mappings");

        if let Some(mut reader) = self.category(CategoryType::FmmuExtended).await? {
            let mut buf = [0u8; { FmmuEx::STORAGE_SIZE }];

            while reader.read(&mut buf).await? == FmmuEx::STORAGE_SIZE {
                let fmmu = FmmuEx::parse(&buf)?;

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
        direction: PdoType,
        valid_range: RangeInclusive<u16>,
    ) -> Result<heapless::Vec<Pdo, 16>, Error> {
        let mut pdos = heapless::Vec::new();

        fmt::trace!("Get {:?} PDUs", direction);

        if let Some(mut reader) = self.category(CategoryType::from(direction)).await? {
            let mut buf = [0u8; { Pdo::STORAGE_SIZE }];

            while reader.read(&mut buf).await? == Pdo::STORAGE_SIZE {
                let mut pdo = Pdo::parse(&buf).map_err(|e| {
                    fmt::error!("PDO: {:?}", e);

                    Error::Eeprom(EepromError::Decode)
                })?;

                fmt::trace!("Range {:?} value {}", valid_range, pdo.index);

                if !valid_range.contains(&pdo.index) {
                    fmt::error!("Invalid PDO range");

                    return Err(Error::Eeprom(EepromError::Decode));
                }

                for _ in 0..pdo.num_entries {
                    let mut buf = [0u8; { PdoEntry::STORAGE_SIZE }];

                    let entry = reader.read_exact(&mut buf).await.and_then(|_| {
                        let entry = PdoEntry::parse(&buf).map_err(|e| {
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
        self.pdos(PdoType::Tx, TX_PDO_RANGE).await
    }

    /// Receive PDOs (from device's perspective) - outputs
    pub(crate) async fn master_write_pdos(&self) -> Result<heapless::Vec<Pdo, 16>, Error> {
        self.pdos(PdoType::Rx, RX_PDO_RANGE).await
    }

    /// Find a string in the device EEPROM.
    ///
    /// An index of 0 denotes an empty string and will always return `Ok(None)`.
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
            let mut buf = [0u8; 1];
            reader.read_exact(&mut buf).await?;
            let num_strings = buf[0];

            fmt::trace!("--> Slave has {} strings", num_strings);

            if search_index > num_strings {
                return Ok(None);
            }

            for i in 0..search_index {
                // let string_len = reader.try_next().await?;
                let mut buf = [0u8; 1];
                reader.read_exact(&mut buf).await?;
                let string_len = buf[0];

                fmt::trace!("String index {} has len {}", i, string_len);

                reader.skip_ahead_bytes(string_len.into()).await?;
            }

            // let string_len = usize::from(reader.try_next().await?);
            let mut buf = [0u8; 1];
            reader.read_exact(&mut buf).await?;
            let string_len = buf[0];

            if usize::from(string_len) > N {
                return Err(Error::StringTooLong {
                    max_length: N,
                    string_length: string_len.into(),
                });
            }

            let mut buf = [0u8; N];
            let mut bytes = &mut buf[0..string_len.into()];

            reader.read_exact(&mut bytes).await?;

            // let bytes = reader
            //     .take_vec_len_exact::<N>(string_len)
            //     .await
            //     .map_err(|_| Error::StringTooLong {
            //         max_length: N,
            //         string_length: string_len,
            //     })?;

            fmt::trace!("--> Raw string bytes {:?}", bytes);

            let s = core::str::from_utf8(&bytes).map_err(|_e| {
                #[cfg(feature = "std")]
                fmt::error!("Invalid UTF8: {}", _e);

                Error::Eeprom(EepromError::Decode)
            })?;

            // Strip trailing null bytes from string.
            // TODO: Unit test this when an EEPROM shim is added
            let s = s.trim_end_matches('\0');

            let s = heapless::String::<N>::from_str(s).map_err(|_| {
                fmt::error!("String too long");

                Error::Eeprom(EepromError::Decode)
            })?;

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        base_data_types::PrimitiveDataType,
        eeprom::{
            file_reader::EepromFile,
            types::{MailboxProtocols, PdoFlags, SyncManagerEnable, SyncManagerType},
        },
        sync_manager_channel::{Control, Direction, OperationMode},
    };

    #[tokio::test]
    async fn read_device_name() {
        let _ = env_logger::builder().is_test(true).try_init();

        let e = SlaveEeprom::new(EepromFile::new("dumps/eeprom/el2889.hex"));

        assert_eq!(
            e.device_name::<64>().await,
            Ok(Some("EL2889".try_into().unwrap()))
        );
    }

    #[tokio::test]
    async fn sync_managers() {
        let _ = env_logger::builder().is_test(true).try_init();

        let e = SlaveEeprom::new(EepromFile::new("dumps/eeprom/akd.hex"));

        let expected = [
            SyncManager {
                start_addr: 0x1800,
                length: 0x0400,
                control: Control {
                    operation_mode: OperationMode::Mailbox,
                    direction: Direction::MasterWrite,
                    ecat_event_enable: false,
                    dls_user_event_enable: true,
                    watchdog_enable: false,
                },
                enable: SyncManagerEnable::ENABLE,
                usage_type: SyncManagerType::MailboxWrite,
            },
            SyncManager {
                start_addr: 0x1c00,
                length: 0x0400,
                control: Control {
                    operation_mode: OperationMode::Mailbox,
                    direction: Direction::MasterRead,
                    ecat_event_enable: false,
                    dls_user_event_enable: true,
                    watchdog_enable: false,
                },
                enable: SyncManagerEnable::ENABLE,
                usage_type: SyncManagerType::MailboxRead,
            },
            SyncManager {
                start_addr: 0x1100,
                length: 0x0000,
                control: Control {
                    operation_mode: OperationMode::Normal,
                    direction: Direction::MasterWrite,
                    ecat_event_enable: false,
                    dls_user_event_enable: true,
                    watchdog_enable: false,
                },
                enable: SyncManagerEnable::ENABLE,
                usage_type: SyncManagerType::ProcessDataWrite,
            },
            SyncManager {
                start_addr: 0x1140,
                length: 0x0000,
                control: Control {
                    operation_mode: OperationMode::Normal,
                    direction: Direction::MasterRead,
                    ecat_event_enable: false,
                    dls_user_event_enable: true,
                    watchdog_enable: false,
                },
                enable: SyncManagerEnable::ENABLE,
                usage_type: SyncManagerType::ProcessDataRead,
            },
        ];

        assert_eq!(
            e.sync_managers().await,
            Ok(heapless::Vec::<SyncManager, 8>::from_slice(&expected).unwrap())
        );
    }

    #[tokio::test]
    async fn empty_string() {
        let _ = env_logger::builder().is_test(true).try_init();

        let e = SlaveEeprom::new(EepromFile::new("dumps/eeprom/el2828.hex"));

        // Ensure we have at least one string.
        assert_eq!(
            e.find_string::<64>(1).await,
            Ok(Some("EL2828".try_into().unwrap()))
        );

        // 0th index always returns None
        assert_eq!(e.find_string::<64>(0).await, Ok(None));
    }

    #[tokio::test]
    async fn short_buffer() {
        let _ = env_logger::builder().is_test(true).try_init();

        let e = SlaveEeprom::new(EepromFile::new("dumps/eeprom/akd.hex"));

        // Pick a decently long string from the EEPROM file. This is just an arbitrary index.
        let idx = 12;

        let expected = "Velocity actual value";

        // Ensure we have at least one string.
        assert_eq!(
            e.find_string::<64>(idx).await,
            Ok(Some(expected.try_into().unwrap())),
            "EEPROM should have at least one string"
        );

        // Reading into a buffer that's too short should error, not truncate or otherwise fail silently
        assert_eq!(
            e.find_string::<8>(idx).await,
            Err(Error::StringTooLong {
                max_length: 8,
                string_length: expected.len(),
            }),
            "Read should fail if buffer is too small"
        );
    }

    #[tokio::test]
    async fn strings() -> Result<(), Error> {
        let _ = env_logger::builder().is_test(true).try_init();

        let e = SlaveEeprom::new(EepromFile::new("dumps/eeprom/akd.hex"));

        let mut strings = Vec::new();

        // EEPROM dump was manually determined to have 34 strings in it.
        let num_strings = 34;

        // 0th string is special empty string index, so start from index 1.
        for idx in 1..num_strings {
            let s = e.find_string::<64>(idx).await?;

            if let Some(s) = s {
                strings.push(s.as_str().to_string());
            }
        }

        // Any strings after the valid index range shouldn't error, but should return nothing.
        assert_eq!(e.find_string::<64>(num_strings + 1).await, Ok(None));

        assert_eq!(
            strings,
            [
                "AKD",
                "Drive",
                "Drives",
                "AKD EtherCAT Drive (CoE)",
                "DRIVE",
                "DcSync",
                "DcOff",
                "Inputs",
                "Statusword",
                "Position actual internal value",
                "Second position feedback",
                "Velocity actual value",
                "Digital inputs",
                "Following error actual value",
                "Latch 1p",
                "Torque actual value",
                "Latch statusword",
                "AIN.VALUE",
                "Latch 1n",
                "Latch 1 pn",
                "Position actual value",
                "Latch 2 pn",
                "Outputs",
                "Controlword",
                "1st set-point",
                "Target velocity",
                "Latch controlword",
                "Torque offset",
                "Physical outputs",
                "Max torque",
                "ClearDigInputChangedBit",
                "Target position",
                "AOUT.VALUE write",
            ]
        );

        Ok(())
    }

    #[tokio::test]
    async fn pdos_invalid_range() {
        let e = SlaveEeprom::new(EepromFile::new("dumps/eeprom/akd.hex"));

        assert_eq!(
            e.pdos(PdoType::Rx, 0x1000..=0x1010).await,
            Err(Error::Eeprom(EepromError::Decode))
        );
    }

    // EK1100 doesn't have any IO so doesn't have any PDOs.
    #[tokio::test]
    async fn slave_no_pdos() {
        let e = SlaveEeprom::new(EepromFile::new("dumps/eeprom/ek1100.hex"));

        assert_eq!(e.master_read_pdos().await, Ok(heapless::Vec::new()));
        assert_eq!(e.master_write_pdos().await, Ok(heapless::Vec::new()));
    }

    #[tokio::test]
    async fn output_pdos_only() {
        let e = SlaveEeprom::new(EepromFile::new("dumps/eeprom/el2828.hex"));

        fn pdo(index: u16, name_string_idx: u8, entry_idx: u16) -> Pdo {
            let entry_defaults = PdoEntry {
                index: 0x7000,
                sub_index: 1,
                name_string_idx: 6,
                data_type: PrimitiveDataType::Bool,
                data_length_bits: 1,
                flags: 0,
            };

            let pdo_defaults = Pdo {
                index: 0x1600,
                name_string_idx: 5,

                num_entries: 1,
                sync_manager: 0,
                dc_sync: 0,
                flags: PdoFlags::PDO_MANDATORY | PdoFlags::PDO_FIXED_CONTENT,

                entries: heapless::Vec::from_slice(&[PdoEntry {
                    index: 0x7000,
                    ..entry_defaults
                }])
                .unwrap(),
            };

            Pdo {
                index,
                name_string_idx,
                entries: heapless::Vec::from_slice(&[PdoEntry {
                    index: entry_idx,
                    ..entry_defaults
                }])
                .unwrap(),
                ..pdo_defaults
            }
        }

        let output_pdos = [
            pdo(0x1600, 5, 0x7000),
            pdo(0x1601, 7, 0x7010),
            pdo(0x1602, 8, 0x7020),
            pdo(0x1603, 9, 0x7030),
            pdo(0x1604, 10, 0x7040),
            pdo(0x1605, 11, 0x7050),
            pdo(0x1606, 12, 0x7060),
            pdo(0x1607, 13, 0x7070),
        ];

        assert_eq!(e.master_read_pdos().await, Ok(heapless::Vec::new()));
        pretty_assertions::assert_eq!(
            e.master_write_pdos().await,
            Ok(heapless::Vec::from_slice(&output_pdos).unwrap())
        );
    }

    // This exercises the "read from a specific address" codepath as opposed to the "find a category
    // and start reading it" codepath.
    #[tokio::test]
    async fn get_mailbox_config() {
        let e = SlaveEeprom::new(EepromFile::new("dumps/eeprom/akd.hex"));

        assert_eq!(
            e.mailbox_config().await,
            Ok(DefaultMailbox {
                slave_receive_offset: 0x1800,
                slave_receive_size: 0x0400,
                slave_send_offset: 0x1c00,
                slave_send_size: 0x0400,
                supported_protocols: MailboxProtocols::EOE
                    | MailboxProtocols::COE
                    | MailboxProtocols::FOE,
            })
        );
    }

    #[tokio::test]
    async fn default_mailbox_config_matches_sms() {
        let e = SlaveEeprom::new(EepromFile::new("dumps/eeprom/akd.hex"));

        let sms = e.sync_managers().await.expect("Read sync managers");

        let mbox = e.mailbox_config().await.expect("Read mailbox config");

        assert_eq!(
            mbox.slave_receive_offset, sms[0].start_addr,
            "slave_receive_offset"
        );
        assert_eq!(mbox.slave_receive_size, sms[0].length, "slave_receive_size");
        assert_eq!(
            mbox.slave_send_offset, sms[1].start_addr,
            "slave_send_offset"
        );
        assert_eq!(mbox.slave_send_size, sms[1].length, "slave_send_size");
    }

    #[tokio::test]
    async fn get_fmmu_usage() {
        assert_eq!(
            SlaveEeprom::new(EepromFile::new("dumps/eeprom/akd.hex"))
                .fmmus()
                .await,
            Ok(heapless::Vec::from_slice(&[
                FmmuUsage::Outputs,
                FmmuUsage::Inputs,
                FmmuUsage::SyncManagerStatus,
                FmmuUsage::Unused,
            ])
            .unwrap())
        );

        assert_eq!(
            SlaveEeprom::new(EepromFile::new("dumps/eeprom/el2828.hex"))
                .fmmus()
                .await,
            Ok(heapless::Vec::from_slice(&[FmmuUsage::Outputs, FmmuUsage::Unused,]).unwrap())
        );
    }

    #[tokio::test]
    async fn no_fmmus() {
        let e = SlaveEeprom::new(EepromFile::new("dumps/eeprom/ek1100.hex"));

        assert_eq!(e.fmmus().await, Ok(heapless::Vec::new()));
    }

    #[tokio::test]
    async fn identity() {
        let e = SlaveEeprom::new(EepromFile::new("dumps/eeprom/akd.hex"));

        assert_eq!(
            e.identity().await,
            Ok(SlaveIdentity {
                vendor_id: 0x0000006a,
                product_id: 0x00414b44,
                revision: 2,
                serial: 2575499411,
            })
        );
    }
}
