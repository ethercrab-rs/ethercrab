use crate::{
    al_control::AlControl,
    al_status_code::AlStatusCode,
    client::Client,
    coe::{services::DownloadExpeditedRequest, CoeHeader, CoeService, InitSdoFlags, InitSdoHeader},
    eeprom::{
        types::{FmmuUsage, SiiOwner, SyncManager, SyncManagerEnable, SyncManagerType},
        Eeprom,
    },
    error::Error,
    fmmu::Fmmu,
    mailbox::{MailboxHeader, MailboxType, Priority},
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

#[derive(Debug)]
pub struct Slave {
    /// Configured station address.
    pub(crate) configured_address: u16,
    /// Index into PDI map corresponding to slave inputs.
    pub(crate) input_range: Option<PdiSegment>,
    /// Index into PDI map corresponding to slave outputs.
    pub(crate) output_range: Option<PdiSegment>,

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
        let slave_ref = SlaveRef::new(client, configured_address);

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
            input_range: None,
            output_range: None,
            identity,
            name,
        })
    }

    pub(crate) fn io_segments(&self) -> (Option<PdiSegment>, Option<PdiSegment>) {
        (self.input_range.clone(), self.output_range.clone())
    }
}

pub struct SlaveRef<'a, const MAX_FRAMES: usize, const MAX_PDU_DATA: usize, TIMEOUT> {
    client: &'a Client<'a, MAX_FRAMES, MAX_PDU_DATA, TIMEOUT>,
    configured_address: u16,
}

impl<'a, const MAX_FRAMES: usize, const MAX_PDU_DATA: usize, TIMEOUT>
    SlaveRef<'a, MAX_FRAMES, MAX_PDU_DATA, TIMEOUT>
where
    TIMEOUT: TimerFactory,
{
    pub fn new(
        client: &'a Client<MAX_FRAMES, MAX_PDU_DATA, TIMEOUT>,
        configured_address: u16,
    ) -> Self {
        Self {
            client,
            configured_address,
        }
    }

    /// A wrapper around an FPRD service to this slave's configured address.
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
    {
        self.client
            .fpwr(self.configured_address, register, value)
            .await?
            .wkc(1, context)
    }

    pub async fn write_sdo<T>(
        &self,
        index: u16,
        sub_index: u8,
        value: T,
        complete_access: bool,
    ) -> Result<(), Error>
    where
        T: PduData,
        <T as PduRead>::Error: Debug,
    {
        // Only expedited requests for now
        if T::len() > 4 {
            // TODO: Error::Sdo(SdoError::DataTooLong) or something
            panic!("Data too long");
        }

        let mut data = [0u8; 4];

        let len = usize::from(T::len());

        data[0..len].copy_from_slice(value.as_slice());

        let request = DownloadExpeditedRequest {
            header: MailboxHeader {
                length: 0x0a,
                address: 0x0000,
                priority: Priority::Lowest,
                mailbox_type: MailboxType::Coe,
                counter: 0,
            },
            coe_header: CoeHeader {
                // TODO: Keep a mailbox counter of 1-7 in `Client` or `PduLoop`. 0 is a reserved value
                number: 1,
                service: CoeService::SdoRequest,
            },
            sdo_header: InitSdoHeader {
                flags: InitSdoFlags {
                    size_indicator: true,
                    expedited_transfer: true,
                    size: (len - 4) as u8,
                    complete_access,
                    command: InitSdoFlags::DOWNLOAD_REQUEST,
                },
                index,
                sub_index,
            },
            data: data,
        };

        let payload = request.pack().unwrap();

        // TODO: Store mailbox read/write address in slave structure
        let response = self.write(0x1800u16, payload, "SDO write").await?;

        dbg!(response);

        Ok(())
    }

    async fn wait_for_state(&self, desired_state: SlaveState) -> Result<(), Error> {
        crate::timeout::<TIMEOUT, _, _>(Duration::from_millis(1000), async {
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
        Eeprom::new(&self)
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
            "Slave {:#06x} SM{sync_manager_index}: {:#?}",
            self.configured_address,
            sm_config
        );

        Ok(sm_config)
    }

    pub(crate) async fn configure_from_eeprom_safe_op(&self) -> Result<(), Error> {
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
    ) -> Result<(PdiOffset, Option<PdiSegment>, Option<PdiSegment>), Error> {
        // RX from the perspective of the slave device
        let master_write_pdos = self.eeprom().master_write_pdos().await?;
        let master_read_pdos = self.eeprom().master_read_pdos().await?;

        let sync_managers = self.eeprom().sync_managers().await?;
        let fmmu_usage = self.eeprom().fmmus().await?;
        let fmmu_sm_mappings = self.eeprom().fmmu_mappings().await?;

        // PDOs must be configurd in PRE-OP state
        // TODO: I think I need to read the PDOs out of CoE (if supported?), not EEPROM
        let outputs_range = self
            .configure_pdos(
                &sync_managers,
                &master_write_pdos,
                &fmmu_sm_mappings,
                &fmmu_usage,
                PdoDirection::MasterWrite,
                &mut offset,
            )
            .await?;

        let inputs_range = self
            .configure_pdos(
                &sync_managers,
                &master_read_pdos,
                &fmmu_sm_mappings,
                &fmmu_usage,
                PdoDirection::MasterRead,
                &mut offset,
            )
            .await?;

        // Restore EEPROM mode
        self.set_eeprom_mode(SiiOwner::Pdi).await?;

        self.request_slave_state(SlaveState::SafeOp).await?;

        // self.slave.input_range = inputs_range.clone();
        // self.slave.output_range = outputs_range.clone();

        Ok((offset, inputs_range, outputs_range))
    }

    async fn configure_mailboxes(&self, sync_managers: &[SyncManager]) -> Result<(), Error> {
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
                }
                SyncManagerType::MailboxRead => {
                    self.write_sm_config(
                        sync_manager_index,
                        sync_manager,
                        mailbox_config.slave_send_size,
                    )
                    .await?;
                }
                _ => continue,
            }
        }

        Ok(())
    }

    /// Configure SM and FMMU mappings for either TX or RX PDOs.
    ///
    /// PDOs must be configured with the slave in PRE-OP state
    async fn configure_pdos(
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

        // Configure output bits first so they're before inputs in the PDI
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

            let sm_config = self
                .write_sm_config(sync_manager_index, sync_manager, (bit_len + 7) / 8)
                .await?;

            let fmmu_config = Fmmu {
                logical_start_address: offset.start_address,
                length_bytes: sm_config.length_bytes,
                // Mapping into PDI is byte-aligned
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
                "Slave {:#06x} FMMU{fmmu_index}: {:#?}",
                self.configured_address,
                fmmu_config
            );

            *offset = offset.increment_byte_aligned(bit_len);
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
