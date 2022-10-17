mod sdo;

use crate::{
    al_control::AlControl,
    al_status_code::AlStatusCode,
    client::Client,
    coe::SdoAccess,
    eeprom::{
        types::{
            FmmuUsage, MailboxProtocols, SiiOwner, SyncManager, SyncManagerEnable, SyncManagerType,
        },
        Eeprom,
    },
    error::Error,
    fmmu::Fmmu,
    pdi::{PdiOffset, PdiSegment},
    pdu_loop::CheckWorkingCounter,
    register::RegisterAddress,
    slave_state::SlaveState,
    sync_manager_channel::{self, SyncManagerChannel},
    timer_factory::TimerFactory,
    PduData, PduRead,
};
use core::fmt::Debug;
use core::fmt::Write;
use core::{fmt, time::Duration};
use nom::{number::complete::le_u32, IResult};
use num_enum::FromPrimitive;
use packed_struct::PackedStruct;

#[derive(Default)]
pub struct SlaveIdentity {
    pub vendor_id: u32,
    pub product_id: u32,
    pub revision: u32,
    pub serial: u32,
}

impl fmt::Debug for SlaveIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SlaveIdentity")
            .field("vendor_id", &format_args!("{:#010x}", self.vendor_id))
            .field("product_id", &format_args!("{:#010x}", self.product_id))
            .field("revision", &self.revision)
            .field("serial", &self.serial)
            .finish()
    }
}

impl SlaveIdentity {
    pub fn parse(i: &[u8]) -> IResult<&[u8], Self> {
        let (i, vendor_id) = le_u32(i)?;
        let (i, product_id) = le_u32(i)?;
        let (i, revision) = le_u32(i)?;
        let (i, serial) = le_u32(i)?;

        Ok((
            i,
            Self {
                vendor_id,
                product_id,
                revision,
                serial,
            },
        ))
    }
}

#[derive(Debug, Default, Clone)]
pub struct SlaveConfig {
    pub io: IoRanges,
    pub mailbox: MailboxConfig,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct MailboxConfig {
    read: Option<Mailbox>,
    write: Option<Mailbox>,
    supported_protocols: MailboxProtocols,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct Mailbox {
    address: u16,
    len: u16,
    sync_manager: u8,
}

#[derive(Debug, Default, Clone)]
pub struct IoRanges {
    pub input: Option<PdiSegment>,
    pub output: Option<PdiSegment>,
}

impl IoRanges {
    pub fn working_counter_sum(&self) -> u16 {
        self.input.as_ref().map(|_| 1).unwrap_or(0) + self.output.as_ref().map(|_| 2).unwrap_or(0)
    }
}

#[derive(Debug)]
pub struct Slave {
    /// Configured station address.
    pub(crate) configured_address: u16,

    pub(crate) config: SlaveConfig,

    pub identity: SlaveIdentity,

    // NOTE: Default length in SOEM is 40 bytes
    pub name: heapless::String<64>,
}

impl Slave {
    pub(crate) async fn new<'client, const MAX_FRAMES: usize, const MAX_PDU_DATA: usize, TIMEOUT>(
        client: &'client Client<'client, MAX_FRAMES, MAX_PDU_DATA, TIMEOUT>,
        configured_address: u16,
    ) -> Result<Self, Error>
    where
        TIMEOUT: TimerFactory,
    {
        let mut config = SlaveConfig::default();

        let slave_ref = SlaveRef::new(client, &mut config, configured_address, "");

        slave_ref.wait_for_state(SlaveState::Init).await?;

        // Will be/should be set to SiiOwner::Pdi after init
        slave_ref.set_eeprom_mode(SiiOwner::Master).await?;

        let eep = slave_ref.eeprom();

        let identity = eep.identity().await?;

        let name = eep.device_name().await?.unwrap_or_else(|| {
            let mut s = heapless::String::new();

            write!(
                s,
                "manu. {:#010x}, device {:#010x}, serial {:#010x}",
                identity.vendor_id, identity.product_id, identity.serial
            )
            .unwrap();

            s
        });

        log::debug!("Slave name {}", name);

        Ok(Self {
            configured_address,
            identity,
            name,
            config,
        })
    }

    pub(crate) fn io_segments(&self) -> &IoRanges {
        &self.config.io
    }
}

pub struct SlaveRef<'a, const MAX_FRAMES: usize, const MAX_PDU_DATA: usize, TIMEOUT> {
    client: &'a Client<'a, MAX_FRAMES, MAX_PDU_DATA, TIMEOUT>,
    pub(crate) config: &'a mut SlaveConfig,
    configured_address: u16,
    name: &'a str,
}

impl<'a, const MAX_FRAMES: usize, const MAX_PDU_DATA: usize, TIMEOUT>
    SlaveRef<'a, MAX_FRAMES, MAX_PDU_DATA, TIMEOUT>
where
    TIMEOUT: TimerFactory,
{
    pub fn new(
        client: &'a Client<MAX_FRAMES, MAX_PDU_DATA, TIMEOUT>,
        config: &'a mut SlaveConfig,
        configured_address: u16,
        name: &'a str,
    ) -> Self {
        Self {
            client,
            config,
            configured_address,
            name,
        }
    }

    pub fn name(&self) -> &str {
        self.name
    }

    pub(crate) async fn read<T>(
        &self,
        register: RegisterAddress,
        context: &'static str,
    ) -> Result<T, Error>
    where
        T: PduRead,
        <T as PduRead>::Error: Debug,
    {
        self.client
            .fprd(self.configured_address, register)
            .await?
            .wkc(1, context)
    }

    /// A wrapper around an FPWR service to this slave's configured address.
    pub(crate) async fn write<T>(
        &self,
        register: impl Into<u16>,
        value: T,
        context: &'static str,
    ) -> Result<T, Error>
    where
        T: PduData,
        <T as PduRead>::Error: Debug,
    {
        self.client
            .fpwr(self.configured_address, register, value)
            .await?
            .wkc(1, context)
    }

    async fn wait_for_state(&self, desired_state: SlaveState) -> Result<(), Error> {
        crate::timeout::<TIMEOUT, _, _>(Duration::from_millis(5000), async {
            loop {
                let status = self
                    .read::<AlControl>(RegisterAddress::AlStatus, "Read AL status")
                    .await?;

                if status.state == desired_state {
                    break Result::<(), _>::Ok(());
                }

                TIMEOUT::timer(Duration::from_millis(10)).await;
            }
        })
        .await
    }

    pub async fn request_slave_state(&self, desired_state: SlaveState) -> Result<(), Error> {
        debug!(
            "Set state {} for slave address {:#04x}",
            desired_state, self.configured_address
        );

        // Send state request
        self.write(
            RegisterAddress::AlControl,
            AlControl::new(desired_state).pack().unwrap(),
            "AL control",
        )
        .await?;

        self.wait_for_state(desired_state).await
    }

    pub async fn status(&self) -> Result<(SlaveState, AlStatusCode), Error> {
        let status = self
            .read::<AlControl>(RegisterAddress::AlStatus, "AL Status")
            .await
            .map(|ctl| ctl.state)?;

        let code = self
            .read::<AlStatusCode>(RegisterAddress::AlStatusCode, "AL Status Code")
            .await?;

        Ok((status, code))
    }

    // TODO: Separate TIMEOUT for EEPROM specifically
    pub fn eeprom(&'a self) -> Eeprom<'a, MAX_FRAMES, MAX_PDU_DATA, TIMEOUT> {
        Eeprom::new(self)
    }

    async fn set_eeprom_mode(&self, mode: SiiOwner) -> Result<(), Error> {
        self.write::<u16>(RegisterAddress::SiiConfig, 2, "debug write")
            .await?;
        self.write::<u16>(RegisterAddress::SiiConfig, mode as u16, "debug write 2")
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

        self.write(
            RegisterAddress::sync_manager(sync_manager_index),
            sm_config.pack().unwrap(),
            "SM config",
        )
        .await?;

        log::debug!(
            "Slave {:#06x} SM{sync_manager_index}: {}",
            self.configured_address,
            sm_config
        );
        log::trace!("{:#?}", sm_config);

        Ok(sm_config)
    }

    pub(crate) async fn configure_from_eeprom_safe_op(&mut self) -> Result<(), Error> {
        // Force EEPROM into master mode. Some slaves require PDI mode for INIT -> PRE-OP
        // transition. This is mentioned in ETG2010 p. 146 under "Eeprom/@AssignToPd". We'll reset
        // to master mode here, now that the transition is complete.
        self.set_eeprom_mode(SiiOwner::Master).await?;

        let sync_managers = self.eeprom().sync_managers().await?;

        // Mailboxes must be configured in INIT state
        self.configure_mailboxes(&sync_managers).await?;

        // Some slaves must be in PDI EEPROM mode to transition from INIT to PRE-OP. This is
        // mentioned in ETG2010 p. 146 under "Eeprom/@AssignToPd"
        self.set_eeprom_mode(SiiOwner::Pdi).await?;

        self.request_slave_state(SlaveState::PreOp).await?;

        self.set_eeprom_mode(SiiOwner::Master).await?;

        Ok(())
    }

    pub(crate) async fn configure_from_eeprom_pre_op(
        &mut self,
        mut offset: PdiOffset,
    ) -> Result<PdiOffset, Error> {
        let master_write_pdos = self.eeprom().master_write_pdos().await?;
        let master_read_pdos = self.eeprom().master_read_pdos().await?;

        log::trace!("Slave RX PDOs {:#?}", master_write_pdos);
        log::trace!("Slave TX PDOs {:#?}", master_read_pdos);

        let sync_managers = self.eeprom().sync_managers().await?;
        let fmmu_usage = self.eeprom().fmmus().await?;
        let fmmu_sm_mappings = self.eeprom().fmmu_mappings().await?;

        let has_coe = self
            .config
            .mailbox
            .supported_protocols
            .contains(MailboxProtocols::COE);

        // PDOs must be configurd in PRE-OP state
        // Outputs are configured first, so will be before inputs in the PDI
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
                &master_write_pdos,
                &fmmu_sm_mappings,
                &fmmu_usage,
                PdoDirection::MasterWrite,
                &mut offset,
            )
            .await?
        };

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
                &master_read_pdos,
                &fmmu_sm_mappings,
                &fmmu_usage,
                PdoDirection::MasterRead,
                &mut offset,
            )
            .await?
        };

        // Restore EEPROM mode
        self.set_eeprom_mode(SiiOwner::Pdi).await?;

        self.request_slave_state(SlaveState::SafeOp).await?;

        self.config.io = IoRanges {
            input: input_range,
            output: output_range,
        };

        Ok(offset)
    }

    async fn configure_mailboxes(&mut self, sync_managers: &[SyncManager]) -> Result<(), Error> {
        // Read default mailbox configuration from slave information area
        let mailbox_config = self.eeprom().mailbox_config().await?;

        log::trace!(
            "Slave {:#06x} Mailbox configuration: {:#?}",
            self.configured_address,
            mailbox_config
        );

        if !mailbox_config.has_mailbox() {
            log::trace!(
                "Slave {:#06x} has no valid mailbox configuration",
                self.configured_address
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

        self.config.mailbox = MailboxConfig {
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
        // SM0 address
        // TODO: Const
        let sm_base_address = 0x1c10;

        let (desired_sm_type, desired_fmmu_type) = direction.filter_terms();

        // ETG1000.6 Table 67 – CoE Communication Area
        let num_sms = self.read_sdo::<u8>(0x1c00, SdoAccess::Index(0)).await?;

        log::trace!("Found {num_sms} SMs from CoE");

        let start_offset = *offset;

        // We must ignore the first two SM indices (SM0, SM1, sub-index 1 and 2, start at sub-index
        // 3) as these are used for mailbox communication.
        let sm_range = 3..=num_sms;

        let mut total_bit_len = 0;

        // NOTE: This is a 1-based SDO sub-index
        for sm_mapping_sub_index in sm_range {
            let sm_type = self
                // TODO: 0x1c00 const: SM_COMM_TYPE
                .read_sdo::<u8>(0x1c00, SdoAccess::Index(sm_mapping_sub_index))
                .await
                .map(|raw| SyncManagerType::from_primitive(raw))?;

            let sync_manager_index = sm_mapping_sub_index - 1;

            let sm_address = sm_base_address + u16::from(sync_manager_index);

            let sync_manager = sync_managers
                .get(usize::from(sync_manager_index))
                // TODO: Better error type
                .ok_or(Error::Other)?;

            if sm_type != desired_sm_type {
                continue;
            }

            // Total number of PDO assignments for this sync manager
            let num_sm_assignments = self.read_sdo::<u8>(sm_address, SdoAccess::Index(0)).await?;

            log::trace!("SDO sync manager {sync_manager_index} (sub index #{sm_mapping_sub_index}) {sm_address:#06x} {sm_type:?}, sub indices: {num_sm_assignments}");

            let mut sm_bit_len = 0u16;

            for i in 1..=num_sm_assignments {
                let pdo = self
                    .read_sdo::<u16>(sm_address, SdoAccess::Index(i))
                    .await?;
                let num_mappings = self.read_sdo::<u8>(pdo, SdoAccess::Index(0)).await?;

                log::trace!("--> #{i} data: {pdo:#06x} ({num_mappings} mappings):");

                for i in 1..=num_mappings {
                    let mapping = self.read_sdo::<u32>(pdo, SdoAccess::Index(i)).await?;

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

            {
                let sm_config = self
                    .write_sm_config(sync_manager_index, sync_manager, (sm_bit_len + 7) / 8)
                    .await?;

                let fmmu_index = fmmu_usage
                    .iter()
                    .position(|usage| *usage == desired_fmmu_type)
                    // TODO: Better error type
                    .ok_or(Error::Other)?;

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

                self.write(
                    RegisterAddress::fmmu(fmmu_index as u8),
                    fmmu_config.pack().unwrap(),
                    "PDI FMMU",
                )
                .await?;

                log::debug!(
                    "Slave {:#06x} FMMU{fmmu_index}: {}",
                    self.configured_address,
                    fmmu_config
                );
                log::trace!("{:#?}", fmmu_config);

                *offset = offset.increment_byte_aligned(sm_bit_len);
            }

            total_bit_len += sm_bit_len;
        }

        Ok((total_bit_len > 0).then_some(PdiSegment {
            bit_len: total_bit_len.into(),
            bytes: start_offset.up_to(*offset),
        }))
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
                .ok_or(Error::Other)?;

            {
                let sm_config = self
                    .write_sm_config(sync_manager_index, sync_manager, (bit_len + 7) / 8)
                    .await?;

                // TODO: Move this FMMU config into a method and dedupe with COE config
                // TODO: I may need to read and mutate any existing FMMU config; slaves with
                // multiple SMs that point to the same FMMU I think will end up clobbering previous
                // config. This also means I need to configure SMs in this loop, and configure the
                // FMMU ONCE after the loop.
                let fmmu_config = Fmmu {
                    logical_start_address: offset.start_address,
                    length_bytes: sm_config.length_bytes,
                    // Mapping into PDI is byte-aligned until/if we support bit-oriented slaves
                    logical_start_bit: 0,
                    // logical_start_bit: offset.start_bit,
                    logical_end_bit: offset.end_bit(bit_len),
                    physical_start_address: sm_config.physical_start_address,
                    physical_start_bit: 0x0,
                    read_enable: sm_type == SyncManagerType::ProcessDataRead,
                    write_enable: sm_type == SyncManagerType::ProcessDataWrite,
                    enable: true,
                };

                self.write(
                    RegisterAddress::fmmu(fmmu_index),
                    fmmu_config.pack().unwrap(),
                    "PDI FMMU",
                )
                .await?;

                log::debug!(
                    "Slave {:#06x} FMMU{fmmu_index}: {}",
                    self.configured_address,
                    fmmu_config
                );
                log::trace!("{:#?}", fmmu_config);

                *offset = offset.increment_byte_aligned(bit_len);
            }
        }

        Ok((total_bit_len > 0).then_some(PdiSegment {
            bit_len: total_bit_len.into(),
            bytes: start_offset.up_to(*offset),
        }))
    }
}

enum PdoDirection {
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
