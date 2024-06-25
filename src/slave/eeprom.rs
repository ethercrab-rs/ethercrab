use crate::{
    eeprom::types::{
        CategoryType, DefaultMailbox, PdoEntry, SiiGeneral, RX_PDO_RANGE, TX_PDO_RANGE,
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
use core::{marker::PhantomData, ops::RangeInclusive};
use embedded_io_async::{Read, ReadExactError};
use ethercrab_wire::{EtherCrabWireRead, EtherCrabWireReadSized, EtherCrabWireSized};

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
    fn start_at(&self, word_addr: u16, len_bytes: u16) -> ChunkReader<P> {
        ChunkReader::new(self.provider.clone(), word_addr, len_bytes / 2)
    }

    /// Search for a given category and return a reader over the bytes contained within the category
    /// if it is found.
    async fn category(&self, category: CategoryType) -> Result<Option<ChunkReader<P>>, Error> {
        let mut reader = self.provider.clone();

        let mut word_addr = SII_FIRST_CATEGORY_START;

        loop {
            let chunk = reader.read_chunk(word_addr).await?;

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
        let mut reader = self.start_at(0x0018, DefaultMailbox::PACKED_LEN as u16);

        fmt::trace!("Get mailbox config");

        let mut buf = DefaultMailbox::buffer();

        reader.read_exact(&mut buf).await?;

        Ok(DefaultMailbox::unpack_from_slice(&buf)?)
    }

    pub(crate) async fn general(&self) -> Result<SiiGeneral, Error> {
        let mut reader = self
            .category(CategoryType::General)
            .await?
            .ok_or(Error::Eeprom(EepromError::NoCategory))?;

        let mut buf = SiiGeneral::buffer();

        reader.read_exact(&mut buf).await?;

        Ok(SiiGeneral::unpack_from_slice(&buf)?)
    }

    pub(crate) async fn identity(&self) -> Result<SlaveIdentity, Error> {
        let mut reader = self.start_at(0x0008, SlaveIdentity::PACKED_LEN as u16);

        fmt::trace!("Get identity");

        let mut buf = SlaveIdentity::buffer();

        reader.read_exact(&mut buf).await?;

        Ok(SlaveIdentity::unpack_from_slice(&buf)?)
    }

    pub(crate) async fn sync_managers(&self) -> Result<heapless::Vec<SyncManager, 8>, Error> {
        let mut sync_managers = heapless::Vec::<_, 8>::new();

        fmt::trace!("Get sync managers");

        let mut cat = self.items::<SyncManager>(CategoryType::SyncManager).await?;

        while let Some(sm) = cat.next().await? {
            sync_managers
                .push(sm)
                .map_err(|_| Error::Capacity(Item::SyncManager))?;
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
                .iter()
                .map(|raw| {
                    FmmuUsage::try_from(*raw).map_err(|e| {
                        #[cfg(feature = "std")]
                        fmt::error!("Failed to decode FmmuUsage: {}", e);

                        e.into()
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

        let mut cat = self.items::<FmmuEx>(CategoryType::FmmuExtended).await?;

        while let Some(fmmu) = cat.next().await? {
            mappings
                .push(fmmu)
                .map_err(|_| Error::Capacity(Item::FmmuEx))?;
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

        let Some(mut reader) = self.category(CategoryType::from(direction)).await? else {
            return Ok(pdos);
        };

        let mut buf = Pdo::buffer();
        let mut entry_buf = PdoEntry::buffer();

        while reader.read(&mut buf).await? == buf.len() {
            let mut pdo = Pdo::unpack_from_slice(&buf).map_err(|e| {
                fmt::error!("PDO: {:?}", e);

                e
            })?;

            fmt::debug!("Discovered PDO:\n{:#?}", pdo);

            fmt::trace!("Range {:?} value {}", valid_range, pdo.index);

            if !valid_range.contains(&pdo.index) {
                fmt::error!("Invalid PDO range");

                return Err(Error::Eeprom(EepromError::Decode));
            }

            for _ in 0..pdo.num_entries {
                let entry = reader.read_exact(&mut entry_buf).await.and_then(|()| {
                    let entry = PdoEntry::unpack_from_slice(&entry_buf).map_err(|e| {
                        fmt::error!("PDO entry: {:?}", e);

                        Error::Eeprom(EepromError::Decode)
                    })?;

                    Ok(entry)
                })?;

                fmt::debug!("--> PDO entry:\n{:#?}", entry);

                pdo.bit_len += u16::from(entry.data_length_bits);
            }

            pdos.push(pdo).map_err(|_| Error::Capacity(Item::Pdo))?;
        }

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
            let num_strings = reader.read_byte().await?;

            fmt::trace!("--> Slave has {} strings", num_strings);

            if search_index > num_strings {
                return Ok(None);
            }

            for i in 0..search_index {
                let string_len = reader.read_byte().await?;

                fmt::trace!("String index {} has len {}", i, string_len);

                reader.skip_ahead_bytes(string_len.into())?;
            }

            let string_len = usize::from(reader.read_byte().await?);

            if string_len > N {
                return Err(Error::StringTooLong {
                    max_length: N,
                    string_length: string_len.into(),
                });
            }

            let mut buf = heapless::Vec::<u8, N>::new();

            // SAFETY: We MUST ensure that `string_len` is less than `N`
            unsafe { buf.set_len(usize::from(string_len)) }

            reader.read_exact(&mut buf).await?;

            fmt::trace!("--> Raw string bytes {:?}", buf);

            // Get rid of any C null terminators
            buf.retain(|char| *char != 0x00);

            // Invariant: EtherCAT "visible string"s are 0x20 to 0x7E
            if !buf.is_ascii() {
                fmt::error!("String at index {} is not valid ASCII", search_index);

                return Err(Error::Eeprom(EepromError::Decode));
            }

            // SAFETY: We've checked the buffer only contains ASCII characters above, so we don't
            // need to check for valid UTF-8.
            let s = unsafe { heapless::String::<N>::from_utf8_unchecked(buf) };

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

    pub(crate) async fn items<T>(
        &self,
        category: CategoryType,
    ) -> Result<CategoryIterator<P, T>, Error>
    where
        T: EtherCrabWireReadSized,
    {
        let Some(reader) = self.category(category).await? else {
            return Ok(CategoryIterator::new(ChunkReader::new(
                self.provider.clone(),
                0,
                0,
            )));
        };

        Ok(CategoryIterator::new(reader))
    }
}

pub struct CategoryIterator<P, T> {
    reader: ChunkReader<P>,
    item: PhantomData<T>,
}

impl<P, T> CategoryIterator<P, T>
where
    T: EtherCrabWireReadSized,
    P: EepromDataProvider,
{
    pub fn new(reader: ChunkReader<P>) -> Self {
        Self {
            reader,
            item: PhantomData,
        }
    }

    pub async fn next(&mut self) -> Result<Option<T>, Error> {
        let mut buf = T::buffer();

        match self.reader.read_exact(buf.as_mut()).await {
            // Reached end of category
            Err(ReadExactError::UnexpectedEof) => return Ok(None),
            Err(ReadExactError::Other(e)) => return Err(e),
            Ok(()) => (),
        }

        Ok(Some(T::unpack_from_slice(buf.as_ref())?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        eeprom::{
            file_reader::EepromFile,
            types::{
                CoeDetails, Flags, MailboxProtocols, PdoFlags, PortStatus, PortStatuses,
                SyncManagerEnable, SyncManagerType,
            },
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
    async fn single_null_terminator() -> Result<(), Error> {
        let _ = env_logger::builder().is_test(true).try_init();

        let e = SlaveEeprom::new(EepromFile::new("dumps/eeprom/akd_null_strings.hex"));

        // Index 4 was originally "AKD EtherCAT Drive (CoE)", now modified to "AKD EtherCA\0"
        let s = e.find_string::<64>(4).await?;

        assert_eq!(s.as_ref().map(|s| s.as_str()), Some("AKD EtherCA"));

        Ok(())
    }

    #[tokio::test]
    async fn null_terminators() -> Result<(), Error> {
        let _ = env_logger::builder().is_test(true).try_init();

        let e = SlaveEeprom::new(EepromFile::new("dumps/eeprom/akd_null_strings.hex"));

        // Index 10 was originally "Statusword", now modified to "Statu\0\0\0\0\0"
        let s = e.find_string::<64>(10).await?;

        assert_eq!(s.as_ref().map(|s| s.as_str()), Some("Statu"));

        Ok(())
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

        fn pdo(index: u16, name_string_idx: u8, _entry_idx: u16) -> Pdo {
            // let entry_defaults = PdoEntry {
            //     index: 0x7000,
            //     sub_index: 1,
            //     name_string_idx: 6,
            //     data_type: PrimitiveDataType::Bool,
            //     data_length_bits: 1,
            //     flags: 0,
            // };

            let pdo_defaults = Pdo {
                index: 0x1600,
                name_string_idx: 5,

                num_entries: 1,
                sync_manager: 0,
                dc_sync: 0,
                flags: PdoFlags::PDO_MANDATORY | PdoFlags::PDO_FIXED_CONTENT,

                bit_len: 1,
                // entries: heapless::Vec::from_slice(&[PdoEntry {
                //     index: 0x7000,
                //     ..entry_defaults
                // }])
                // .unwrap(),
            };

            Pdo {
                index,
                name_string_idx,

                bit_len: 1,
                // entries: heapless::Vec::from_slice(&[PdoEntry {
                //     index: entry_idx,
                //     ..entry_defaults
                // }])
                // .unwrap(),
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

    #[tokio::test]
    async fn get_general_akd() {
        let e = SlaveEeprom::new(EepromFile::new("dumps/eeprom/akd.hex"));

        assert_eq!(
            e.general().await,
            Ok(SiiGeneral {
                group_string_idx: 2,
                image_string_idx: 5,
                order_string_idx: 1,
                name_string_idx: 4,
                coe_details: CoeDetails::ENABLE_SDO
                    | CoeDetails::ENABLE_PDO_ASSIGN
                    | CoeDetails::ENABLE_PDO_CONFIG,
                foe_enabled: true,
                eoe_enabled: true,
                flags: Flags::ENABLE_SAFE_OP | Flags::MAILBOX_DLL,
                ebus_current: 0,
                ports: PortStatuses([
                    PortStatus::Ebus,
                    PortStatus::Unused,
                    PortStatus::Unused,
                    PortStatus::Unused,
                ]),
                physical_memory_addr: 17,
            }),
        );
    }

    #[tokio::test]
    async fn get_general_ek1100() {
        let e = SlaveEeprom::new(EepromFile::new("dumps/eeprom/ek1100.hex"));

        assert_eq!(
            e.general().await,
            Ok(SiiGeneral {
                group_string_idx: 2,
                image_string_idx: 0,
                order_string_idx: 1,
                name_string_idx: 4,
                coe_details: CoeDetails::empty(),
                foe_enabled: false,
                eoe_enabled: false,
                flags: Flags::empty(),
                ebus_current: -2000,
                ports: PortStatuses([
                    PortStatus::Ebus,
                    PortStatus::Unused,
                    PortStatus::Unused,
                    PortStatus::Unused,
                ]),
                physical_memory_addr: 305,
            }),
        );
    }

    #[tokio::test]
    async fn akd_strings() {
        let e = SlaveEeprom::new(EepromFile::new("dumps/eeprom/akd.hex"));

        let general = e.general().await.expect("Get general");

        let group = e.find_string::<128>(general.group_string_idx).await;
        let image = e.find_string::<128>(general.image_string_idx).await;
        let order = e.find_string::<128>(general.order_string_idx).await;
        let name = e.find_string::<128>(general.name_string_idx).await;

        assert_eq!(group, Ok(Some("Drive".try_into().unwrap())));
        assert_eq!(image, Ok(Some("DRIVE".try_into().unwrap())));
        assert_eq!(order, Ok(Some("AKD".try_into().unwrap())));
        assert_eq!(
            name,
            Ok(Some("AKD EtherCAT Drive (CoE)".try_into().unwrap()))
        );
    }

    #[tokio::test]
    async fn ek1100_string_no_image() {
        let e = SlaveEeprom::new(EepromFile::new("dumps/eeprom/ek1100.hex"));

        let general = e.general().await.expect("Get general");

        assert_eq!(general.image_string_idx, 0);

        let image = e.find_string::<128>(general.image_string_idx).await;

        assert_eq!(image, Ok(None));
    }

    #[tokio::test]
    async fn akd_fmmu_ex() {
        let e = SlaveEeprom::new(EepromFile::new("dumps/eeprom/akd.hex"));

        let fmmu_ex = e.fmmu_mappings().await.expect("Get FMMU_EX");

        // None of the EEPROM dumps I have contain any FMMU_EX records :(
        assert_eq!(fmmu_ex, heapless::Vec::<FmmuEx, 16>::new());
    }
}
