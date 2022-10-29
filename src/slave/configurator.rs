//! Slave configuration reading from EEPROM, and SDOs if the slave device supports CANOpen over
//! EtherCAT (CoE).

use super::{slave_client::SlaveClient, Slave, SlaveRef};
use crate::{
    coe::SubIndex,
    eeprom::types::{
        FmmuUsage, MailboxProtocols, SiiOwner, SyncManager, SyncManagerEnable, SyncManagerType,
    },
    error::{Error, Item},
    fmmu::Fmmu,
    pdi::{PdiOffset, PdiSegment},
    pdu_data::{PduData, PduRead},
    register::{RegisterAddress, SupportFlags},
    slave::{IoRanges, Mailbox, MailboxConfig},
    slave_state::SlaveState,
    sync_manager_channel::{self, SyncManagerChannel, SM_BASE_ADDRESS, SM_TYPE_ADDRESS},
    timer_factory::TimerFactory,
    Client,
};
use core::fmt::Debug;
use num_enum::FromPrimitive;
use packed_struct::PackedStruct;

pub struct SlaveConfigurator<'a, const MAX_FRAMES: usize, const MAX_PDU_DATA: usize, TIMEOUT> {
    client: SlaveClient<'a, MAX_FRAMES, MAX_PDU_DATA, TIMEOUT>,
    slave: &'a mut Slave,
}

impl<'a, const MAX_FRAMES: usize, const MAX_PDU_DATA: usize, TIMEOUT>
    SlaveConfigurator<'a, MAX_FRAMES, MAX_PDU_DATA, TIMEOUT>
where
    TIMEOUT: TimerFactory,
{
    pub fn new(
        client: &'a Client<'a, MAX_FRAMES, MAX_PDU_DATA, TIMEOUT>,
        slave: &'a mut Slave,
    ) -> Self {
        Self {
            client: SlaveClient::new(client, slave.configured_address),
            slave,
        }
    }

    /// First stage configuration (INIT -> PRE-OP).
    ///
    /// Continue configuration by calling [`configure_fmmus`](SlaveConfigurator::configure_fmmus)
    pub(crate) async fn configure_mailboxes(&mut self) -> Result<(), Error> {
        // Force EEPROM into master mode. Some slaves require PDI mode for INIT -> PRE-OP
        // transition. This is mentioned in ETG2010 p. 146 under "Eeprom/@AssignToPd". We'll reset
        // to master mode here, now that the transition is complete.
        self.client.set_eeprom_mode(SiiOwner::Master).await?;

        let sync_managers = self.client.eeprom().sync_managers().await?;

        // Mailboxes must be configured in INIT state
        self.configure_mailbox_sms(&sync_managers).await?;

        // Some slaves must be in PDI EEPROM mode to transition from INIT to PRE-OP. This is
        // mentioned in ETG2010 p. 146 under "Eeprom/@AssignToPd"
        self.client.set_eeprom_mode(SiiOwner::Pdi).await?;

        self.client.request_slave_state(SlaveState::PreOp).await?;

        self.client.set_eeprom_mode(SiiOwner::Master).await?;

        Ok(())
    }

    /// Second state configuration (PRE-OP -> SAFE-OP).
    ///
    /// PDOs must be configured in the PRE-OP state.
    pub(crate) async fn configure_fmmus(
        &mut self,
        mut offset: PdiOffset,
        direction: PdoDirection,
    ) -> Result<PdiOffset, Error> {
        let sync_managers = self.client.eeprom().sync_managers().await?;
        let fmmu_usage = self.client.eeprom().fmmus().await?;
        let fmmu_sm_mappings = self.client.eeprom().fmmu_mappings().await?;

        // TODO: Add an assertion that slave is in PRE-OP

        let has_coe = self
            .slave
            .config
            .mailbox
            .supported_protocols
            .contains(MailboxProtocols::COE)
            && self
                .slave
                .config
                .mailbox
                .read
                .map(|mbox| mbox.len > 0)
                .unwrap_or(false);

        log::debug!("Slave {:#06x} has CoE", self.slave.configured_address);

        match direction {
            PdoDirection::MasterRead => {
                let pdos = self.client.eeprom().master_read_pdos().await?;

                log::trace!("Slave TX PDOs {:#?}", pdos);

                let input_range = if has_coe {
                    self.configure_pdos_coe(
                        &sync_managers,
                        &fmmu_usage,
                        PdoDirection::MasterRead,
                        &mut offset,
                    )
                    .await?
                } else {
                    self.configure_pdos_eeprom(
                        &sync_managers,
                        &pdos,
                        &fmmu_sm_mappings,
                        &fmmu_usage,
                        PdoDirection::MasterRead,
                        &mut offset,
                    )
                    .await?
                };

                self.slave.config.io.input = input_range;
            }
            PdoDirection::MasterWrite => {
                let pdos = self.client.eeprom().master_write_pdos().await?;

                log::trace!("Slave RX PDOs {:#?}", pdos);

                let output_range = if has_coe {
                    self.configure_pdos_coe(
                        &sync_managers,
                        &fmmu_usage,
                        PdoDirection::MasterWrite,
                        &mut offset,
                    )
                    .await?
                } else {
                    self.configure_pdos_eeprom(
                        &sync_managers,
                        &pdos,
                        &fmmu_sm_mappings,
                        &fmmu_usage,
                        PdoDirection::MasterWrite,
                        &mut offset,
                    )
                    .await?
                };

                self.slave.config.io.output = output_range;
            }
        }

        Ok(offset)
    }

    pub async fn request_safe_op_nowait(&self) -> Result<(), Error> {
        // Restore EEPROM mode
        self.client.set_eeprom_mode(SiiOwner::Pdi).await?;

        self.client
            .request_slave_state_nowait(SlaveState::SafeOp)
            .await?;

        Ok(())
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
            status: Default::default(),
            enable: sync_manager_channel::Enable {
                enable: sync_manager.enable.contains(SyncManagerEnable::ENABLE),
                ..Default::default()
            },
        };

        self.client
            .write(
                RegisterAddress::sync_manager(sync_manager_index),
                sm_config.pack().unwrap(),
                "SM config",
            )
            .await?;

        log::debug!(
            "Slave {:#06x} SM{sync_manager_index}: {}",
            self.slave.configured_address,
            sm_config
        );
        log::trace!("{:#?}", sm_config);

        Ok(sm_config)
    }

    /// Configure SM0 and SM1 for mailbox communication.
    async fn configure_mailbox_sms(&mut self, sync_managers: &[SyncManager]) -> Result<(), Error> {
        // Read default mailbox configuration from slave information area
        let mailbox_config = self.client.eeprom().mailbox_config().await?;

        log::trace!(
            "Slave {:#06x} Mailbox configuration: {:#?}",
            self.slave.configured_address,
            mailbox_config
        );

        if !mailbox_config.has_mailbox() {
            log::trace!(
                "Slave {:#06x} has no valid mailbox configuration",
                self.slave.configured_address
            );

            return Ok(());
        }

        let mut read_mailbox = None;
        let mut write_mailbox = None;

        for (sync_manager_index, sync_manager) in sync_managers.iter().enumerate() {
            let sync_manager_index = sync_manager_index as u8;

            // Mailboxes are configured in INIT state
            match sync_manager.usage_type {
                SyncManagerType::MailboxWrite => {
                    self.write_sm_config(
                        sync_manager_index,
                        sync_manager,
                        mailbox_config.slave_receive_size,
                    )
                    .await?;

                    write_mailbox = Some(Mailbox {
                        address: sync_manager.start_addr,
                        len: mailbox_config.slave_receive_size,
                        sync_manager: sync_manager_index,
                    });
                }
                SyncManagerType::MailboxRead => {
                    self.write_sm_config(
                        sync_manager_index,
                        sync_manager,
                        mailbox_config.slave_send_size,
                    )
                    .await?;

                    read_mailbox = Some(Mailbox {
                        address: sync_manager.start_addr,
                        len: mailbox_config.slave_receive_size,
                        sync_manager: sync_manager_index,
                    });
                }
                _ => continue,
            }
        }

        self.slave.config.mailbox = MailboxConfig {
            read: read_mailbox,
            write: write_mailbox,
            supported_protocols: mailbox_config.supported_protocols,
        };

        Ok(())
    }

    /// Configure PDOs from CoE registers.
    async fn configure_pdos_coe(
        &self,
        sync_managers: &[SyncManager],
        fmmu_usage: &[FmmuUsage],
        direction: PdoDirection,
        offset: &mut PdiOffset,
    ) -> Result<Option<PdiSegment>, Error> {
        let (desired_sm_type, desired_fmmu_type) = direction.filter_terms();

        // ETG1000.6 Table 67 – CoE Communication Area
        let num_sms = self
            .read_sdo::<u8>(SM_TYPE_ADDRESS, SubIndex::Index(0))
            .await?;

        log::trace!("Found {num_sms} SMs from CoE");

        let start_offset = *offset;

        // We must ignore the first two SM indices (SM0, SM1, sub-index 1 and 2, start at sub-index
        // 3) as these are used for mailbox communication.
        let sm_range = 3..=num_sms;

        let mut total_bit_len = 0;

        // NOTE: This is a 1-based SDO sub-index
        for sm_mapping_sub_index in sm_range {
            let sm_type = self
                .read_sdo::<u8>(SM_TYPE_ADDRESS, SubIndex::Index(sm_mapping_sub_index))
                .await
                .map(|raw| SyncManagerType::from_primitive(raw))?;

            let sync_manager_index = sm_mapping_sub_index - 1;

            let sm_address = SM_BASE_ADDRESS + u16::from(sync_manager_index);

            let sync_manager =
                sync_managers
                    .get(usize::from(sync_manager_index))
                    .ok_or(Error::NotFound {
                        item: Item::SyncManager,
                        index: Some(usize::from(sync_manager_index)),
                    })?;

            if sm_type != desired_sm_type {
                continue;
            }

            // Total number of PDO assignments for this sync manager
            let num_sm_assignments = self.read_sdo::<u8>(sm_address, SubIndex::Index(0)).await?;

            log::trace!("SDO sync manager {sync_manager_index} (sub index #{sm_mapping_sub_index}) {sm_address:#06x} {sm_type:?}, sub indices: {num_sm_assignments}");

            let mut sm_bit_len = 0u16;

            for i in 1..=num_sm_assignments {
                let pdo = self.read_sdo::<u16>(sm_address, SubIndex::Index(i)).await?;
                let num_mappings = self.read_sdo::<u8>(pdo, SubIndex::Index(0)).await?;

                log::trace!("--> #{i} data: {pdo:#06x} ({num_mappings} mappings):");

                for i in 1..=num_mappings {
                    let mapping = self.read_sdo::<u32>(pdo, SubIndex::Index(i)).await?;

                    // Yes, big-endian. Makes life easier when mapping from debug prints to actual
                    // data fields.
                    let parts = mapping.to_be_bytes();

                    let index = u16::from_be_bytes(parts[0..=1].try_into().unwrap());
                    let sub_index = parts[2];
                    let mapping_bit_len = parts[3];

                    log::trace!(
                        "----> index {index:#06x}, sub index {sub_index}, bit length {mapping_bit_len}"
                    );

                    sm_bit_len += u16::from(mapping_bit_len);
                }
            }

            log::trace!(
                "----= total SM bit length {sm_bit_len} ({} bytes)",
                (sm_bit_len + 7) / 8
            );

            let fmmu_index = fmmu_usage
                .iter()
                .position(|usage| *usage == desired_fmmu_type)
                .ok_or(Error::NotFound {
                    item: Item::Fmmu,
                    index: None,
                })?;

            self.write_fmmu_config(
                sync_manager_index,
                sync_manager,
                sm_bit_len,
                fmmu_index,
                offset,
                total_bit_len,
                desired_sm_type,
            )
            .await?;

            total_bit_len += sm_bit_len;
        }

        Ok((total_bit_len > 0).then_some(PdiSegment {
            bit_len: total_bit_len.into(),
            bytes: start_offset.up_to(*offset),
        }))
    }

    async fn write_fmmu_config(
        &self,
        sync_manager_index: u8,
        sync_manager: &SyncManager,
        sm_bit_len: u16,
        fmmu_index: usize,
        offset: &mut PdiOffset,
        total_bit_len: u16,
        desired_sm_type: SyncManagerType,
    ) -> Result<(), Error> {
        let sm_config = self
            .write_sm_config(sync_manager_index, sync_manager, (sm_bit_len + 7) / 8)
            .await?;

        let fmmu_config = Fmmu {
            logical_start_address: offset.start_address,
            length_bytes: sm_config.length_bytes,
            // Mapping into PDI is byte-aligned until/if we support bit-oriented slaves
            logical_start_bit: 0,
            // logical_start_bit: offset.start_bit,
            logical_end_bit: offset.end_bit(total_bit_len),
            physical_start_address: sm_config.physical_start_address,
            physical_start_bit: 0x0,
            read_enable: desired_sm_type == SyncManagerType::ProcessDataRead,
            write_enable: desired_sm_type == SyncManagerType::ProcessDataWrite,
            enable: true,
        };
        self.client
            .write(
                RegisterAddress::fmmu(fmmu_index as u8),
                fmmu_config.pack().unwrap(),
                "PDI FMMU",
            )
            .await?;
        log::debug!(
            "Slave {:#06x} FMMU{fmmu_index}: {}",
            self.slave.configured_address,
            fmmu_config
        );
        log::trace!("{:#?}", fmmu_config);
        *offset = offset.increment_byte_aligned(sm_bit_len);
        Ok(())
    }

    /// Configure PDOs from EEPROM
    async fn configure_pdos_eeprom(
        &self,
        sync_managers: &[SyncManager],
        pdos: &[crate::eeprom::types::Pdo],
        fmmu_sm_mappings: &[crate::eeprom::types::FmmuEx],
        fmmu_usage: &[FmmuUsage],
        direction: PdoDirection,
        offset: &mut PdiOffset,
    ) -> Result<Option<PdiSegment>, Error> {
        let start_offset = *offset;
        let mut total_bit_len = 0;

        let (sm_type, fmmu_type) = direction.filter_terms();

        for (sync_manager_index, sync_manager) in sync_managers
            .iter()
            .enumerate()
            .filter(|(_idx, sm)| sm.usage_type == sm_type)
        {
            let sync_manager_index = sync_manager_index as u8;

            let bit_len = pdos
                .iter()
                .filter(|pdo| pdo.sync_manager == sync_manager_index)
                .map(|pdo| pdo.bit_len())
                .sum();

            total_bit_len += bit_len;

            // Look for FMMU index using FMMU_EX section in EEPROM. If it's empty, default
            // to looking through FMMU usage list and picking out the appropriate kind
            // (Inputs, Outputs)
            let fmmu_index = fmmu_sm_mappings
                .iter()
                .find(|fmmu| fmmu.sync_manager == sync_manager_index)
                .map(|fmmu| fmmu.sync_manager)
                .or_else(|| {
                    log::trace!("Could not find FMMU for PDO SM{sync_manager_index}");

                    fmmu_usage
                        .iter()
                        .position(|usage| *usage == fmmu_type)
                        .map(|idx| {
                            log::trace!("Using fallback FMMU FMMU{idx}");

                            idx as u8
                        })
                })
                .ok_or(Error::NotFound {
                    item: Item::Fmmu,
                    index: None,
                })?;

            self.write_fmmu_config(
                sync_manager_index,
                sync_manager,
                bit_len,
                usize::from(fmmu_index),
                offset,
                total_bit_len,
                sm_type,
            )
            .await?;
        }

        Ok((total_bit_len > 0).then_some(PdiSegment {
            bit_len: total_bit_len.into(),
            bytes: start_offset.up_to(*offset),
        }))
    }

    pub(crate) fn as_ref(&self) -> SlaveRef<'_, MAX_FRAMES, MAX_PDU_DATA, TIMEOUT> {
        SlaveRef::new(
            SlaveClient::new(self.client.client, self.slave.configured_address),
            self.slave,
        )
    }

    pub async fn read_sdo<T>(&self, index: u16, access: SubIndex) -> Result<T, Error>
    where
        T: PduData,
        <T as PduRead>::Error: Debug,
    {
        self.as_ref().read_sdo(index, access).await
    }
}

pub(crate) enum PdoDirection {
    MasterRead,
    MasterWrite,
}

impl PdoDirection {
    fn filter_terms(self) -> (SyncManagerType, FmmuUsage) {
        match self {
            PdoDirection::MasterRead => (SyncManagerType::ProcessDataRead, FmmuUsage::Inputs),
            PdoDirection::MasterWrite => (SyncManagerType::ProcessDataWrite, FmmuUsage::Outputs),
        }
    }
}
