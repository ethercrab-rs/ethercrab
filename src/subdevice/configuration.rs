use heapless::Entry;

use super::{SubDevice, SubDeviceRef};
use crate::{
    coe::{SdoExpedited, SubIndex},
    ds402::SyncManagerAssignment,
    eeprom::types::{
        CoeDetails, DefaultMailbox, FmmuEx, FmmuUsage, MailboxProtocols, SiiGeneral, SiiOwner,
        SyncManager, SyncManagerEnable, SyncManagerType,
    },
    error::{Error, IgnoreNoCategory, Item},
    fmmu::Fmmu,
    fmt,
    pdi::{PdiOffset, PdiSegment},
    register::RegisterAddress,
    subdevice::types::{Mailbox, MailboxConfig},
    subdevice_group::{FmmuConfig, FmmuKind, MappingConfig},
    subdevice_state::SubDeviceState,
    sync_manager_channel::{Enable, SM_BASE_ADDRESS, SM_TYPE_ADDRESS, Status, SyncManagerChannel},
};
use core::{f32::consts::E, ops::DerefMut};

/// Configuation from EEPROM methods.
impl<S> SubDeviceRef<'_, S>
where
    S: DerefMut<Target = SubDevice>,
{
    /// First stage configuration (INIT -> PRE-OP).
    ///
    /// Continue configuration by calling
    /// [`configure_fmmus`](crate::SubDeviceGroup::configure_fmmus).
    pub(crate) async fn configure_mailboxes(&mut self) -> Result<(), Error> {
        // Force EEPROM into master mode. Some SubDevices require PDI mode for INIT -> PRE-OP
        // transition. This is mentioned in ETG2010 p. 146 under "Eeprom/@AssignToPd". We'll reset
        // to master mode here, now that the transition is complete.
        self.set_eeprom_mode(SiiOwner::Master).await?;

        let sync_managers = self.eeprom().sync_managers().await?;

        // Mailboxes must be configured in INIT state
        self.configure_mailbox_sms(&sync_managers).await?;

        // Some SubDevices must be in PDI EEPROM mode to transition from INIT to PRE-OP. This is
        // mentioned in ETG2010 p. 146 under "Eeprom/@AssignToPd"
        self.set_eeprom_mode(SiiOwner::Pdi).await?;

        fmt::debug!(
            "SubDevice {:#06x} mailbox SMs configured. Transitioning to PRE-OP",
            self.configured_address
        );

        self.request_subdevice_state(SubDeviceState::PreOp).await?;

        if self.state.config.mailbox.has_coe {
            // TODO: Abstract this no-complete-access check into a method call so we can reuse it.
            // CA is currently only used here inside EtherCrab, but may need to be used in other
            // places eventually.
            let sms = if self.state.config.mailbox.complete_access {
                // Up to 16 sync managers as per ETG1000.4 Table 59
                self.sdo_read::<heapless::Vec<SyncManagerType, 16>>(
                    SM_TYPE_ADDRESS,
                    SubIndex::Complete,
                )
                .await?
            } else {
                let num_indices = self
                    .sdo_read_expedited::<u8>(SM_TYPE_ADDRESS, SubIndex::Index(0))
                    .await?;

                let mut sms = heapless::Vec::new();

                for index in 1..=num_indices {
                    let sm = self
                        .sdo_read_expedited::<SyncManagerType>(
                            SM_TYPE_ADDRESS,
                            SubIndex::Index(index),
                        )
                        .await?;

                    fmt::trace!("Sync manager {:?} at sub-index {}", sm, index);

                    sms.push(sm).map_err(|_| {
                        fmt::error!("More than 16 sync manager types deteced");

                        Error::Capacity(Item::SyncManager)
                    })?;
                }

                sms
            };

            fmt::debug!(
                "SubDevice {:#06x} found sync managers {:?}",
                self.configured_address,
                sms
            );

            self.state.config.mailbox.coe_sync_manager_types = sms;
        }

        self.set_eeprom_mode(SiiOwner::Master).await?;

        Ok(())
    }

    /// Second state configuration (PRE-OP -> SAFE-OP).
    ///
    /// PDOs must be configured in the PRE-OP state.
    pub(crate) async fn configure_fmmus(
        &mut self,
        mut global_offset: PdiOffset,
        group_start_address: u32,
        direction: PdoDirection,
        config: &Option<MappingConfig<'_>>,
    ) -> Result<PdiOffset, Error> {
        let eeprom = self.eeprom();

        // TODO: Don't read the EEPROM at all if we have a manual config
        let sync_managers = eeprom.sync_managers().await?;
        let fmmu_usage = eeprom.fmmus().await?;

        let state = self.state().await?;

        if state != SubDeviceState::PreOp {
            fmt::error!(
                "SubDevice {:#06x} is in invalid state {}. Expected {}",
                self.configured_address,
                state,
                SubDeviceState::PreOp
            );

            return Err(Error::InvalidState {
                expected: SubDeviceState::PreOp,
                actual: state,
                configured_address: self.configured_address,
            });
        }

        let has_coe = self.state.config.mailbox.has_coe;

        fmt::debug!(
            "SubDevice {:#06x} has CoE: {:?}",
            self.configured_address,
            has_coe
        );

        let range = if let Some(config) = config {
            self.configure_pdos_config(direction, &mut global_offset, config)
                .await?
        } else if has_coe {
            self.configure_pdos_coe(&sync_managers, &fmmu_usage, direction, &mut global_offset)
                .await?
        } else {
            self.configure_pdos_eeprom(&sync_managers, direction, &mut global_offset)
                .await?
        };

        match direction {
            PdoDirection::MainDeviceRead => {
                self.state.config.io.input = PdiSegment {
                    bytes: (range.bytes.start - group_start_address as usize)
                        ..(range.bytes.end - group_start_address as usize),
                };
            }
            PdoDirection::MainDeviceWrite => {
                self.state.config.io.output = PdiSegment {
                    bytes: (range.bytes.start - group_start_address as usize)
                        ..(range.bytes.end - group_start_address as usize),
                };
            }
        };

        fmt::debug!(
            "SubDevice {:#06x} PDI inputs: {:?} ({} bytes), outputs: {:?} ({} bytes)",
            self.configured_address,
            self.state.config.io.input,
            self.state.config.io.input.len(),
            self.state.config.io.output,
            self.state.config.io.output.len(),
        );

        Ok(global_offset)
    }

    async fn write_sm_config(
        &self,
        sync_manager_index: u8,
        sync_manager: &SyncManager,
        length_bytes: u16,
    ) -> Result<SyncManagerChannel, Error> {
        let sm_config = SyncManagerChannel {
            physical_start_address: sync_manager.start_addr,
            // Bit length, rounded up to the nearest byte
            length_bytes,
            control: sync_manager.control,
            status: Status::default(),
            enable: Enable {
                enable: sync_manager.enable.contains(SyncManagerEnable::ENABLE),
                ..Enable::default()
            },
        };

        self.write(RegisterAddress::sync_manager(sync_manager_index))
            .send(self.maindevice, &sm_config)
            .await?;

        fmt::debug!(
            "SubDevice {:#06x} SM{}: {}",
            self.configured_address,
            sync_manager_index,
            sm_config
        );
        fmt::trace!("{:#?}", sm_config);

        Ok(sm_config)
    }

    /// Configure SM0 and SM1 for mailbox communication.
    async fn configure_mailbox_sms(&mut self, sync_managers: &[SyncManager]) -> Result<(), Error> {
        let eeprom = self.eeprom();

        // Read default mailbox configuration from SubDevice information area
        let mailbox_config = eeprom
            .mailbox_config()
            .await
            .ignore_no_category()?
            .unwrap_or_else(|| {
                fmt::debug!(
                    "{:#06x} has no EEPROM mailbox config, using default",
                    self.configured_address()
                );

                DefaultMailbox::default()
            });

        let general = eeprom
            .general()
            .await
            .ignore_no_category()?
            .unwrap_or_else(|| {
                fmt::debug!(
                    "{:#06x} has no EEPROM general category, using default",
                    self.configured_address()
                );

                SiiGeneral::default()
            });

        fmt::trace!(
            "SubDevice {:#06x} Mailbox configuration: {:#?}",
            self.configured_address,
            mailbox_config
        );

        if !mailbox_config.has_mailbox() {
            fmt::trace!(
                "SubDevice {:#06x} has no valid mailbox configuration",
                self.configured_address
            );

            return Ok(());
        }

        let mut read_mailbox = None;
        let mut write_mailbox = None;

        // NOTE: SOEM defaults SM0 to start 0x1000, size 0x0080 and SM1 to 0x1080/0x0080 if mailbox
        // SMs can't be found.

        for (sync_manager_index, sync_manager) in sync_managers.iter().enumerate() {
            let sync_manager_index = sync_manager_index as u8;

            // Mailboxes are configured in INIT state
            match sync_manager.usage_type() {
                SyncManagerType::MailboxWrite => {
                    self.write_sm_config(
                        sync_manager_index,
                        sync_manager,
                        mailbox_config.subdevice_receive_size,
                    )
                    .await?;

                    write_mailbox = Some(Mailbox {
                        address: sync_manager.start_addr,
                        len: mailbox_config.subdevice_receive_size,
                        sync_manager: sync_manager_index,
                    });
                }
                SyncManagerType::MailboxRead => {
                    self.write_sm_config(
                        sync_manager_index,
                        sync_manager,
                        mailbox_config.subdevice_send_size,
                    )
                    .await?;

                    read_mailbox = Some(Mailbox {
                        address: sync_manager.start_addr,
                        len: mailbox_config.subdevice_send_size,
                        sync_manager: sync_manager_index,
                    });
                }
                _ => continue,
            }
        }

        self.state.config.mailbox = MailboxConfig {
            read: read_mailbox,
            write: write_mailbox,
            supported_protocols: mailbox_config.supported_protocols,
            coe_sync_manager_types: heapless::Vec::new(),
            has_coe: mailbox_config
                .supported_protocols
                .contains(MailboxProtocols::COE)
                && read_mailbox.is_some_and(|mbox| mbox.len > 0),
            complete_access: general
                .coe_details
                .contains(CoeDetails::ENABLE_COMPLETE_ACCESS),
        };

        Ok(())
    }

    /// Configure PDOs from CoE registers.
    async fn configure_pdos_coe(
        &self,
        sync_managers: &[SyncManager],
        fmmu_usage: &[FmmuUsage],
        direction: PdoDirection,
        global_offset: &mut PdiOffset,
    ) -> Result<PdiSegment, Error> {
        if !self.state.config.mailbox.has_coe {
            fmt::warn!("Invariant: attempting to configure PDOs from COE with no SOE support");
        }

        let (desired_sm_type, desired_fmmu_type) = direction.filter_terms();

        // NOTE: Commented out because this causes a timeout on various SubDevices, possibly due
        // to querying 0x1c00 after we enter PRE-OP but I'm unsure. See
        // <https://github.com/ethercrab-rs/ethercrab/issues/49>. Complete access also causes the
        // same issue.
        // // ETG1000.6 Table 67 – CoE Communication Area
        // let num_sms = self
        //     .sdo_read::<u8>(SM_TYPE_ADDRESS, SubIndex::Index(0))
        //     .await?;

        let start_offset = *global_offset;
        // let mut total_bit_len = 0;

        for (sync_manager_index, (sm_type, sync_manager)) in self
            .state
            .config
            .mailbox
            .coe_sync_manager_types
            .iter()
            .zip(sync_managers.iter())
            .enumerate()
        {
            let sync_manager_index = sync_manager_index as u8;

            let sm_address = SM_BASE_ADDRESS + u16::from(sync_manager_index);

            if *sm_type != desired_sm_type {
                continue;
            }

            // Total number of PDO assignments for this sync manager
            let num_sm_assignments = self
                .sdo_read_expedited::<u8>(sm_address, SubIndex::Index(0))
                .await?;

            fmt::trace!(
                "SDO sync manager {}  {:#06x} {:?}, sub indices: {}",
                sync_manager_index,
                sm_address,
                sm_type,
                num_sm_assignments
            );

            let mut sm_bit_len = 0u16;

            for i in 1..=num_sm_assignments {
                let pdo = self
                    .sdo_read_expedited::<u16>(sm_address, SubIndex::Index(i))
                    .await?;
                let num_mappings = self
                    .sdo_read_expedited::<u8>(pdo, SubIndex::Index(0))
                    .await?;

                fmt::trace!(
                    "--> {:#04x} data: {:#06x} ({} mappings):",
                    i,
                    pdo,
                    num_mappings
                );

                for i in 1..=num_mappings {
                    /// Defined in ETG1000.6 Table 74/Table 75 Receive PDO Mapping.
                    ///
                    /// Note that this struct order is opposite to the specification as the data is
                    /// big-endian in EEPROM, but little endian on the wire.
                    #[derive(ethercrab_wire::EtherCrabWireRead)]
                    #[wire(bytes = 4)]
                    struct Mapping {
                        #[wire(bytes = 1)]
                        mapping_bit_len: u8,
                        #[wire(bytes = 1)]
                        sub_index: u8,
                        #[wire(bytes = 2)]
                        index: u16,
                    }

                    impl SdoExpedited for Mapping {}

                    let Mapping {
                        index,
                        sub_index,
                        mapping_bit_len,
                    } = self
                        .sdo_read_expedited::<Mapping>(pdo, SubIndex::Index(i))
                        .await?;

                    fmt::trace!(
                        "----> index {:#06x}, sub index {}, bit length {}",
                        index,
                        sub_index,
                        mapping_bit_len,
                    );

                    sm_bit_len += u16::from(mapping_bit_len);
                }
            }

            fmt::trace!(
                "----= total SM bit length {} ({} bytes)",
                sm_bit_len,
                (sm_bit_len + 7) / 8
            );

            let sm_config = self
                .write_sm_config(sync_manager_index, sync_manager, (sm_bit_len + 7) / 8)
                .await?;

            if sm_bit_len > 0 {
                let fmmu_index = fmmu_usage
                    .iter()
                    .position(|usage| *usage == desired_fmmu_type)
                    .ok_or(Error::NotFound {
                        item: Item::Fmmu,
                        index: None,
                    })?;

                self.write_fmmu_config(
                    fmmu_index as u8,
                    global_offset,
                    desired_sm_type,
                    &sm_config,
                )
                .await?;
            }

            // total_bit_len += sm_bit_len;
        }

        Ok(PdiSegment {
            // bit_len: total_bit_len.into(),
            bytes: start_offset.up_to(*global_offset),
        })
    }

    async fn write_fmmu_config(
        &self,
        fmmu_index: u8,
        global_offset: &mut PdiOffset,
        desired_sm_type: SyncManagerType,
        sm_config: &SyncManagerChannel,
    ) -> Result<(), Error> {
        // Multiple SMs may use the same FMMU, so we'll read the existing config from the SubDevice
        let mut fmmu_config = self
            .read(RegisterAddress::fmmu(fmmu_index))
            .receive::<Fmmu>(self.maindevice)
            .await?;

        // We can use the enable flag as a sentinel for existing config because EtherCrab inits
        // FMMUs to all zeroes on startup.
        let fmmu_config = if fmmu_config.enable {
            fmmu_config.length_bytes += sm_config.length_bytes;

            fmmu_config
        } else {
            Fmmu {
                logical_start_address: global_offset.start_address,
                length_bytes: sm_config.length_bytes,
                // Mapping into PDI is byte-aligned until/if we support bit-oriented SubDevices
                logical_start_bit: 0,
                // Always byte-aligned
                logical_end_bit: 7,
                physical_start_address: sm_config.physical_start_address,
                physical_start_bit: 0x0,
                read_enable: desired_sm_type == SyncManagerType::ProcessDataRead,
                write_enable: desired_sm_type == SyncManagerType::ProcessDataWrite,
                enable: true,
            }
        };

        self.write(RegisterAddress::fmmu(fmmu_index))
            .send(self.maindevice, &fmmu_config)
            .await?;

        fmt::debug!(
            "SubDevice {:#06x} FMMU{}: {}",
            self.configured_address,
            fmmu_index,
            fmmu_config
        );

        *global_offset = global_offset.increment(sm_config.length_bytes);

        Ok(())
    }

    /// Configure PDOs from EEPROM
    async fn configure_pdos_eeprom(
        &self,
        sync_managers: &[SyncManager],
        direction: PdoDirection,
        offset: &mut PdiOffset,
    ) -> Result<PdiSegment, Error> {
        let eeprom = self.eeprom();

        let pdos = match direction {
            PdoDirection::MainDeviceRead => {
                let read_pdos = eeprom.maindevice_read_pdos().await?;

                fmt::trace!("SubDevice inputs PDOs {:#?}", read_pdos);

                read_pdos
            }
            PdoDirection::MainDeviceWrite => {
                let write_pdos = eeprom.maindevice_write_pdos().await?;

                fmt::trace!("SubDevice outputs PDOs {:#?}", write_pdos);

                write_pdos
            }
        };

        let fmmu_sm_mappings = eeprom.fmmu_mappings().await?;

        let start_offset = *offset;

        let (sm_type, _fmmu_type) = direction.filter_terms();

        for (sync_manager_index, sync_manager) in sync_managers
            .iter()
            .enumerate()
            .filter(|(_idx, sm)| sm.usage_type() == sm_type)
        {
            let sync_manager_index = sync_manager_index as u8;

            let bit_len: u16 = pdos
                .iter()
                .filter(|pdo| pdo.sync_manager == sync_manager_index)
                .map(|pdo| pdo.bit_len)
                .sum();

            // Look for FMMU index using FMMU_EX section in EEPROM. If it's empty, default
            // to looking through FMMU usage list and picking out the appropriate kind
            // (Inputs, Outputs)
            let fmmu_index = fmmu_sm_mappings
                .iter()
                .find(|fmmu| fmmu.sync_manager == sync_manager_index)
                .map(|fmmu| fmmu.sync_manager)
                .unwrap_or_else(|| {
                    fmt::trace!(
                        "Could not find FMMU for PDO SM{} in EEPROM, using SM index to pick FMMU instead",
                        sync_manager_index,
                    );

                    sync_manager_index
                });

            let sm_config = self
                .write_sm_config(sync_manager_index, sync_manager, (bit_len + 7) / 8)
                .await?;

            fmt::debug!(
                "{:?} assignment SM {}, FMMU {}",
                sm_type,
                sync_manager_index,
                fmmu_index
            );

            self.write_fmmu_config(fmmu_index, offset, sm_type, &sm_config)
                .await?;
        }

        Ok(PdiSegment {
            bytes: start_offset.up_to(*offset),
        })
    }

    /// Configure PDOs from a given config.
    async fn configure_pdos_config(
        &self,
        direction: PdoDirection,
        offset: &mut PdiOffset,
        config: &MappingConfig<'_>,
    ) -> Result<PdiSegment, Error> {
        fmt::debug!(
            "Configure SubDevice {:#06x} Sync Managers and FMMUs from given config",
            self.configured_address()
        );

        let start_offset = *offset;

        let eeprom = self.eeprom();

        let sync_managers = if !config.sync_managers.is_empty() {
            config
                .sync_managers
                .iter()
                .map(|sm| sm.bikeshed_into_eeprom_type())
                .collect()
        } else {
            // Fall back to trying to read sync managers from EEPROM if none were specified in the
            // config. This list may be empty, in which case future code should map FMMUs and SMs by
            // equal index.
            eeprom.sync_managers().await?
        };

        let fmmu_sm_mappings = if !config.fmmus.is_empty() {
            config
                .fmmus
                .iter()
                .filter_map(|fmmu| fmmu.sync_manager.map(|sm| FmmuEx { sync_manager: sm }))
                .collect()
        } else {
            // Fall back to trying to read sync managers from EEPROM if none were specified in the
            // config. This list may be empty, in which case future code should map FMMUs and SMs by
            // equal index.
            eeprom.fmmu_mappings().await?
        };

        let objects = match direction {
            PdoDirection::MainDeviceRead => config.inputs,
            PdoDirection::MainDeviceWrite => config.outputs,
        };

        let sm_type = match direction {
            PdoDirection::MainDeviceRead => SyncManagerType::ProcessDataRead,
            PdoDirection::MainDeviceWrite => SyncManagerType::ProcessDataWrite,
        };

        #[derive(Debug)]
        struct ConfigBikeshed {
            sync_manager: SyncManager,
            fmmu_index: u8,
        }

        let mut configuration = heapless::FnvIndexMap::<u8, ConfigBikeshed, 16>::new();

        for (i, assignment) in objects.iter().enumerate() {
            // SM index is either:
            // - sm_config field which explicitly chooses one by index
            // - If that's not populated, iterate and find an SM by type = PD and direction = the
            //   direction we've been given in the fn args. For multiple SMs with the same type,
            //   this will just find the first one but we can't do anything else.
            let (sync_manager_index, sync_manager) = assignment
                .sync_manager
                .and_then(|sm_index| {
                    let sm = sync_managers.get(usize::from(sm_index))?;

                    Some((sm_index, sm))
                })
                .or_else(|| {
                    sync_managers
                        .iter()
                        .enumerate()
                        .find(|(_idx, sm)| sm.usage_type() == sm_type)
                        .map(|(idx, sm)| (idx as u8, sm))
                })
                .ok_or_else(|| {
                    fmt::error!(
                        "Failed to find sync manager for {:?} assignment at index {}",
                        direction,
                        i
                    );

                    Error::NotFound {
                        item: Item::SyncManager,
                        index: Some(i),
                    }
                })?;

            let fmmu_index = fmmu_sm_mappings
                .iter()
                .find(|fmmu| fmmu.sync_manager == sync_manager_index)
                .map(|fmmu| fmmu.sync_manager)
                .unwrap_or_else(|| {
                    fmt::trace!(
                        "Could not find FMMU for PDO SM{} in EEPROM, using SM index to pick FMMU{} instead",
                        sync_manager_index,
                        sync_manager_index,
                    );

                    sync_manager_index
                });

            // Takes into account oversampling if configured
            let byte_len = assignment.len_bytes();

            match configuration.entry(sync_manager_index) {
                Entry::Occupied(mut config) => {
                    let config = config.get_mut();

                    // It should stay the same if everything is working correctly
                    debug_assert_eq!(config.fmmu_index, fmmu_index);

                    // Just increment size
                    config.sync_manager.length_bytes += byte_len;
                }
                Entry::Vacant(vacant_entry) => {
                    let mut sync_manager = *sync_manager;

                    sync_manager.length_bytes = byte_len;

                    // The unwrap assumes the number of entries never goes above 16 here
                    fmt::unwrap!(vacant_entry.insert(ConfigBikeshed {
                        sync_manager,
                        fmmu_index,
                    }));
                }
            }
        }

        for (sm_index, config) in configuration.into_iter() {
            fmt::debug!(
                "--> Configuring {:?} to SM{}, FMMU{}, {} bytes",
                config.sync_manager.usage_type(),
                sm_index,
                config.fmmu_index,
                config.sync_manager.length_bytes
            );

            let sm_config = self
                .write_sm_config(
                    sm_index,
                    &config.sync_manager,
                    config.sync_manager.length_bytes,
                )
                .await?;

            self.write_fmmu_config(config.fmmu_index, offset, sm_type, &sm_config)
                .await?;
        }

        Ok(PdiSegment {
            bytes: start_offset.up_to(*offset),
        })
    }
}

#[derive(Debug, Copy, Clone)]
pub enum PdoDirection {
    MainDeviceRead,
    MainDeviceWrite,
}

impl PdoDirection {
    fn filter_terms(self) -> (SyncManagerType, FmmuUsage) {
        match self {
            PdoDirection::MainDeviceRead => (SyncManagerType::ProcessDataRead, FmmuUsage::Inputs),
            PdoDirection::MainDeviceWrite => {
                (SyncManagerType::ProcessDataWrite, FmmuUsage::Outputs)
            }
        }
    }
}
