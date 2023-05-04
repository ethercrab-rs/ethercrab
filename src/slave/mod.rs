mod eeprom;
pub mod ports;

use crate::{
    al_control::AlControl,
    al_status_code::AlStatusCode,
    all_consumed,
    client::Client,
    coe::SubIndex,
    coe::{
        self,
        abort_code::AbortCode,
        services::{CoeServiceRequest, CoeServiceResponse},
    },
    command::Command,
    dl_status::DlStatus,
    eeprom::types::FromEeprom,
    eeprom::types::{
        FmmuEx, FmmuUsage, MailboxProtocols, Pdo, SiiOwner, SyncManager, SyncManagerEnable,
        SyncManagerType,
    },
    error::{Error, Item, MailboxError, PduError},
    fmmu::Fmmu,
    mailbox::MailboxType,
    pdi::PdiOffset,
    pdi::PdiSegment,
    pdu_data::{PduData, PduRead},
    pdu_loop::{CheckWorkingCounter, PduResponse, RxFrameDataBuf},
    register::RegisterAddress,
    register::SupportFlags,
    slave::ports::{Port, Ports},
    slave_state::SlaveState,
    sync_manager_channel::SyncManagerChannel,
    sync_manager_channel::{self, SM_BASE_ADDRESS, SM_TYPE_ADDRESS},
    Timeouts,
};
use core::{
    any::type_name,
    borrow::Borrow,
    fmt::{self, Debug, Write},
};
use nom::{bytes::complete::take, number::complete::le_u32, IResult};
use num_enum::FromPrimitive;
use packed_struct::{PackedStruct, PackedStructInfo, PackedStructSlice};

#[derive(Default, Copy, Clone, PartialEq)]
pub struct SlaveIdentity {
    pub vendor_id: u32,
    pub product_id: u32,
    pub revision: u32,
    pub serial: u32,
}

impl Debug for SlaveIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SlaveIdentity")
            .field("vendor_id", &format_args!("{:#010x}", self.vendor_id))
            .field("product_id", &format_args!("{:#010x}", self.product_id))
            .field("revision", &self.revision)
            .field("serial", &self.serial)
            .finish()
    }
}

impl FromEeprom for SlaveIdentity {
    const STORAGE_SIZE: usize = 16;

    fn parse_fields(i: &[u8]) -> IResult<&[u8], Self> {
        let (i, vendor_id) = le_u32(i)?;
        let (i, product_id) = le_u32(i)?;
        let (i, revision) = le_u32(i)?;
        let (i, serial) = le_u32(i)?;

        all_consumed(i)?;

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

#[derive(Debug, Default, Clone, PartialEq)]
pub struct SlaveConfig {
    pub io: IoRanges,
    pub mailbox: MailboxConfig,
}

#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct MailboxConfig {
    read: Option<Mailbox>,
    write: Option<Mailbox>,
    supported_protocols: MailboxProtocols,
}

#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct Mailbox {
    address: u16,
    len: u16,
    sync_manager: u8,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct IoRanges {
    pub input: PdiSegment,
    pub output: PdiSegment,
}

impl IoRanges {
    /// Expected working counter value for this slave.
    ///
    /// The working counter is calculated as follows:
    ///
    /// - If the slave has input data, increment by 1
    /// - If the slave has output data, increment by 2
    fn working_counter_sum(&self) -> u16 {
        let l = self.input.len().min(1) + (self.output.len().min(1) * 2);

        l as u16
    }
}

#[derive(Debug, Clone, PartialEq)]
// Gated by test feature so we can easily create test cases, but not expose a `Default`-ed `Slave`
// to the user as this is an invalid state.
#[cfg_attr(test, derive(Default))]
pub struct Slave {
    /// Configured station address.
    pub(crate) configured_address: u16,

    pub(crate) config: SlaveConfig,

    pub identity: SlaveIdentity,

    // NOTE: Default length in SOEM is 40 bytes
    pub name: heapless::String<64>,

    pub(crate) flags: SupportFlags,

    pub(crate) ports: Ports,

    /// Distributed Clock latch receive time.
    pub(crate) dc_receive_time: i64,

    /// The index of the slave in the EtherCAT tree.
    pub(crate) index: usize,

    /// The index of the previous slave in the EtherCAT tree.
    ///
    /// For the first slave in the network, this will always be `None`.
    pub(crate) parent_index: Option<usize>,

    /// Propagation delay in nanoseconds.
    ///
    /// `u32::MAX` gives a maximum propagation delay of ~4.2 seconds for the last slave in the
    /// network.
    pub(crate) propagation_delay: u32,
}

impl Slave {
    /// Create a slave instance using the given configured address.
    ///
    /// This method reads the slave's name and other identifying information, but does not configure
    /// the slave.
    pub(crate) async fn new<'sto>(
        client: &'sto Client<'sto>,
        index: usize,
        configured_address: u16,
    ) -> Result<Self, Error> {
        // let slave_ref = SlaveClient::new(client, configured_address);

        // let mut this =;

        let slave_ref = SlaveRef::new(client, configured_address, ());

        slave_ref.wait_for_state(SlaveState::Init).await?;

        // Make sure master has access to slave EEPROM
        slave_ref.set_eeprom_mode(SiiOwner::Master).await?;

        let identity = slave_ref.eeprom_identity().await?;

        let name = slave_ref.eeprom_device_name().await?.unwrap_or_else(|| {
            let mut s = heapless::String::new();

            write!(
                s,
                "manu. {:#010x}, device {:#010x}, serial {:#010x}",
                identity.vendor_id, identity.product_id, identity.serial
            )
            .unwrap();

            s
        });

        let flags = slave_ref
            .read::<SupportFlags>(RegisterAddress::SupportFlags, "support flags")
            .await?;

        let ports = slave_ref
            .read::<DlStatus>(RegisterAddress::DlStatus, "DL status")
            .await
            .map(|dl_status| {
                // NOTE: dc_receive_times are populated during DC initialisation
                Ports([
                    Port {
                        number: 0,
                        active: dl_status.link_port0,
                        ..Default::default()
                    },
                    Port {
                        number: 1,
                        active: dl_status.link_port1,
                        ..Default::default()
                    },
                    Port {
                        number: 2,
                        active: dl_status.link_port2,
                        ..Default::default()
                    },
                    Port {
                        number: 3,
                        active: dl_status.link_port3,
                        ..Default::default()
                    },
                ])
            })?;

        log::debug!("Slave {:#06x} name {}", configured_address, name);

        Ok(Self {
            configured_address,
            config: SlaveConfig::default(),
            index,
            parent_index: None,
            propagation_delay: 0,
            dc_receive_time: 0,
            identity,
            name,
            flags,
            ports,
        })

        // this.identity = identity;
        // this.name = name;
        // this.flags = flags;
        // this.ports = ports;

        // Ok(this)
    }

    pub(crate) fn io_segments(&self) -> &IoRanges {
        &self.config.io
    }
}

#[derive(Debug)]
pub struct SlaveRef<'a, S> {
    client: &'a Client<'a>,
    // slave: &'a Slave,
    configured_address: u16,
    state: S,
}

/// Items that only need a shared reference to the slave, mainly SDO functions.
impl<'a, S> SlaveRef<'a, S>
where
    S: Borrow<Slave>,
{
    pub fn name(&self) -> &str {
        self.state.borrow().name.as_str()
    }

    /// Send a mailbox request, wait for response mailbox to be ready, read response from mailbox
    /// and return as a slice.
    async fn send_coe_service<H>(
        &self,
        request: H,
    ) -> Result<(H::Response, RxFrameDataBuf<'_>), Error>
    where
        H: CoeServiceRequest,
        <H as PackedStruct>::ByteArray: AsRef<[u8]>,
    {
        let write_mailbox = self
            .state
            .borrow()
            .config
            .mailbox
            .write
            .ok_or(Error::Mailbox(MailboxError::NoMailbox))?;
        let read_mailbox = self
            .state
            .borrow()
            .config
            .mailbox
            .read
            .ok_or(Error::Mailbox(MailboxError::NoMailbox))?;

        let counter = request.counter();

        // TODO: Abstract this into a method that returns a slice
        self.client
            .pdu_loop
            .pdu_tx_readwrite_len(
                Command::Fpwr {
                    address: self.configured_address,
                    register: write_mailbox.address,
                },
                request.pack().unwrap().as_ref(),
                write_mailbox.len,
            )
            .await?
            .wkc(1, "SDO upload request")?;

        // Wait for slave send mailbox to be ready
        crate::timer_factory::timeout(self.client.timeouts.mailbox_echo, async {
            let mailbox_read_sm = RegisterAddress::sync_manager(read_mailbox.sync_manager);

            loop {
                let sm = self
                    .read::<SyncManagerChannel>(mailbox_read_sm, "Master read mailbox")
                    .await?;

                if sm.status.mailbox_full {
                    break Ok(());
                }

                self.client.timeouts.loop_tick().await;
            }
        })
        .await
        .map_err(|e| {
            log::error!("Mailbox read ready error: {e:?}");

            e
        })?;

        // Receive data from slave send mailbox
        // TODO: Abstract this into a method that returns a slice
        let mut response = self
            .client
            .pdu_loop
            .pdu_tx_readonly(
                Command::Fprd {
                    address: self.configured_address,
                    register: read_mailbox.address,
                },
                read_mailbox.len,
            )
            .await?
            .wkc(1, "SDO read mailbox")?;

        // TODO: Retries. Refer to SOEM's `ecx_mbxreceive` for inspiration

        let headers_len = H::Response::packed_bits() / 8;

        let (headers, data) = response.split_at(headers_len);

        let headers = H::Response::unpack_from_slice(headers).map_err(|e| {
            log::error!("Failed to unpack mailbox response headers: {e}");

            e
        })?;

        if headers.is_aborted() {
            let code = data[0..4]
                .try_into()
                .map(|arr| AbortCode::from(u32::from_le_bytes(arr)))
                .map_err(|_| {
                    log::error!("Not enough data to decode abort code u32");

                    Error::Internal
                })?;

            Err(Error::Mailbox(MailboxError::Aborted {
                code,
                address: headers.address(),
                sub_index: headers.sub_index(),
            }))
        }
        // Validate that the mailbox response is to the request we just sent
        // TODO: Determine if we need to check the counter. I don't think SOEM does, it might just
        // be used by the slave?
        else if headers.mailbox_type() != MailboxType::Coe
        /* || headers.counter() != counter */
        {
            log::error!(
                "Invalid SDO response. Type: {:?} (expected {:?}), counter {} (expected {})",
                headers.mailbox_type(),
                MailboxType::Coe,
                headers.counter(),
                counter
            );

            Err(Error::Mailbox(MailboxError::SdoResponseInvalid {
                address: headers.address(),
                sub_index: headers.sub_index(),
            }))
        } else {
            response.trim_front(headers_len);
            Ok((headers, response))
        }
    }

    pub async fn write_sdo<T>(&self, index: u16, sub_index: SubIndex, value: T) -> Result<(), Error>
    where
        T: PduData,
        <T as PduRead>::Error: Debug,
    {
        let counter = self.client.mailbox_counter();

        if T::len() > 4 {
            // TODO: Normal SDO download. Only expedited requests for now
            panic!("Data too long");
        }

        let mut data = [0u8; 4];

        let len = usize::from(T::len());

        data[0..len].copy_from_slice(value.as_slice());

        let request = coe::services::download(counter, index, sub_index, data, len as u8);

        log::trace!("CoE download");

        let (_response, _data) = self.send_coe_service(request).await?;

        // TODO: Validate reply?

        Ok(())
    }

    async fn read_sdo_buf<'buf>(
        &self,
        index: u16,
        sub_index: SubIndex,
        buf: &'buf mut [u8],
    ) -> Result<&'buf [u8], Error> {
        let request = coe::services::upload(self.client.mailbox_counter(), index, sub_index);

        log::trace!("CoE upload");

        let (headers, response) = self.send_coe_service(request).await?;
        let data: &[u8] = &response;

        // Expedited transfers where the data is 4 bytes or less long, denoted in the SDO header
        // size value.
        if headers.sdo_header.flags.expedited_transfer {
            let data_len = 4usize.saturating_sub(usize::from(headers.sdo_header.flags.size));
            let data = &data[0..data_len];

            let buf = &mut buf[0..data_len];

            buf.copy_from_slice(data);

            Ok(buf)
        }
        // Data is either a normal upload or a segmented upload
        else {
            let data_length = headers.header.length.saturating_sub(0x0a);

            let (data, complete_size) = le_u32(data)?;

            // The provided buffer isn't long enough to contain all mailbox data.
            if complete_size > buf.len() as u32 {
                return Err(Error::Mailbox(MailboxError::TooLong {
                    address: headers.address(),
                    sub_index: headers.sub_index(),
                }));
            }

            // If it's a normal upload, the response payload is returned in the initial mailbox read
            if complete_size <= u32::from(data_length) {
                let (_rest, data) = take(data_length)(data)?;

                buf.copy_from_slice(data);

                Ok(&buf[0..usize::from(data_length)])
            }
            // If it's a segmented upload, we must make subsequent requests to load all segment data
            // from the read mailbox.
            else {
                let mut toggle = false;
                let mut total_len = 0usize;

                loop {
                    let request =
                        coe::services::upload_segmented(self.client.mailbox_counter(), toggle);

                    log::trace!("CoE upload segmented");

                    let (headers, data) = self.send_coe_service(request).await?;

                    // The spec defines the data length as n-3, so we'll just go with that magic
                    // number...
                    let mut chunk_len = usize::from(headers.header.length - 3);

                    // Special case as per spec: Minimum response size is 7 bytes. For smaller
                    // responses, we must remove the number of unused bytes at the end of the
                    // response. Extremely weird.
                    if chunk_len == 7 {
                        chunk_len -= usize::from(headers.sdo_header.segment_data_size);
                    }

                    let data = &data[0..chunk_len];

                    buf[total_len..][..chunk_len].copy_from_slice(data);
                    total_len += chunk_len;

                    if headers.sdo_header.is_last_segment {
                        break;
                    }

                    toggle = !toggle;
                }

                Ok(&buf[0..total_len])
            }
        }
    }

    pub async fn read_sdo<T>(&self, index: u16, sub_index: SubIndex) -> Result<T, Error>
    where
        T: PduData,
        <T as PduRead>::Error: Debug,
    {
        // FIXME: Make this dynamic somehow
        let mut buf = [0u8; 32];

        self.read_sdo_buf(index, sub_index, &mut buf)
            .await
            .and_then(|data| {
                T::try_from_slice(data).map_err(|_| {
                    log::error!(
                        "SDO expedited data decode T: {} (len {}) data {:?} (len {})",
                        type_name::<T>(),
                        T::len(),
                        data,
                        data.len()
                    );

                    Error::Pdu(PduError::Decode)
                })
            })
    }

    pub(crate) fn working_counter_sum(&self) -> u16 {
        self.state.borrow().config.io.working_counter_sum()
    }
}

/// EEPROM configuration methods.
impl<'a> SlaveRef<'a, &'a mut Slave> {
    /// First stage configuration (INIT -> PRE-OP).
    ///
    /// Continue configuration by calling [`configure_fmmus`](SlaveConfigurator::configure_fmmus)
    pub(crate) async fn configure_mailboxes(&mut self) -> Result<(), Error> {
        // Force EEPROM into master mode. Some slaves require PDI mode for INIT -> PRE-OP
        // transition. This is mentioned in ETG2010 p. 146 under "Eeprom/@AssignToPd". We'll reset
        // to master mode here, now that the transition is complete.
        self.set_eeprom_mode(SiiOwner::Master).await?;

        let sync_managers = self.eeprom_sync_managers().await?;

        // Mailboxes must be configured in INIT state
        self.configure_mailbox_sms(&sync_managers).await?;

        // Some slaves must be in PDI EEPROM mode to transition from INIT to PRE-OP. This is
        // mentioned in ETG2010 p. 146 under "Eeprom/@AssignToPd"
        self.set_eeprom_mode(SiiOwner::Pdi).await?;

        self.request_slave_state(SlaveState::PreOp).await?;

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
    ) -> Result<PdiOffset, Error> {
        let sync_managers = self.eeprom_sync_managers().await?;
        let fmmu_usage = self.eeprom_fmmus().await?;
        let fmmu_sm_mappings = self.eeprom_fmmu_mappings().await?;

        let (state, _status_code) = self.status().await?;

        if state != SlaveState::PreOp {
            log::error!(
                "Slave {:#06x} is in invalid state {}. Expected {}",
                self.configured_address,
                state,
                SlaveState::PreOp
            );

            return Err(Error::InvalidState {
                expected: SlaveState::PreOp,
                actual: state,
                configured_address: self.configured_address,
            });
        }

        let has_coe = self
            .state
            .config
            .mailbox
            .supported_protocols
            .contains(MailboxProtocols::COE)
            && self
                .state
                .config
                .mailbox
                .read
                .map(|mbox| mbox.len > 0)
                .unwrap_or(false);

        log::debug!(
            "Slave {:#06x} has CoE: {has_coe:?}",
            self.configured_address
        );

        match direction {
            PdoDirection::MasterRead => {
                let pdos = self.eeprom_master_read_pdos().await?;

                log::trace!("Slave inputs PDOs {:#?}", pdos);

                let input_range = if has_coe {
                    self.configure_pdos_coe(
                        &sync_managers,
                        &fmmu_usage,
                        PdoDirection::MasterRead,
                        &mut global_offset,
                    )
                    .await?
                } else {
                    self.configure_pdos_eeprom(
                        &sync_managers,
                        &pdos,
                        &fmmu_sm_mappings,
                        &fmmu_usage,
                        PdoDirection::MasterRead,
                        &mut global_offset,
                    )
                    .await?
                };

                self.state.config.io.input = PdiSegment {
                    bytes: (input_range.bytes.start - group_start_address as usize)
                        ..(input_range.bytes.end - group_start_address as usize),
                    ..input_range
                };
            }
            PdoDirection::MasterWrite => {
                let pdos = self.eeprom_master_write_pdos().await?;

                log::trace!("Slave outputs PDOs {:#?}", pdos);

                let output_range = if has_coe {
                    self.configure_pdos_coe(
                        &sync_managers,
                        &fmmu_usage,
                        PdoDirection::MasterWrite,
                        &mut global_offset,
                    )
                    .await?
                } else {
                    self.configure_pdos_eeprom(
                        &sync_managers,
                        &pdos,
                        &fmmu_sm_mappings,
                        &fmmu_usage,
                        PdoDirection::MasterWrite,
                        &mut global_offset,
                    )
                    .await?
                };

                self.state.config.io.output = PdiSegment {
                    bytes: (output_range.bytes.start - group_start_address as usize)
                        ..(output_range.bytes.end - group_start_address as usize),
                    ..output_range
                };
            }
        }

        log::debug!(
            "Slave {:#06x} PDI inputs: {:?} ({} bytes), outputs: {:?} ({} bytes)",
            self.configured_address,
            self.state.config.io.input,
            self.state.config.io.input.len(),
            self.state.config.io.output,
            self.state.config.io.output.len(),
        );

        Ok(global_offset)
    }

    pub async fn request_safe_op_nowait(&self) -> Result<(), Error> {
        // Restore EEPROM mode
        self.set_eeprom_mode(SiiOwner::Pdi).await?;

        self.request_slave_state_nowait(SlaveState::SafeOp).await?;

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

    /// Configure SM0 and SM1 for mailbox communication.
    async fn configure_mailbox_sms(&mut self, sync_managers: &[SyncManager]) -> Result<(), Error> {
        // Read default mailbox configuration from slave information area
        let mailbox_config = self.eeprom_mailbox_config().await?;

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

        self.state.config.mailbox = MailboxConfig {
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
        gobal_offset: &mut PdiOffset,
    ) -> Result<PdiSegment, Error> {
        let (desired_sm_type, desired_fmmu_type) = direction.filter_terms();

        // ETG1000.6 Table 67 – CoE Communication Area
        let num_sms = self
            .read_sdo::<u8>(SM_TYPE_ADDRESS, SubIndex::Index(0))
            .await?;

        log::trace!("Found {num_sms} SMs from CoE");

        let start_offset = *gobal_offset;

        // We must ignore the first two SM indices (SM0, SM1, sub-index 1 and 2, start at sub-index
        // 3) as these are used for mailbox communication.
        let sm_range = 3..=num_sms;

        let mut total_bit_len = 0;

        // NOTE: This is a 1-based SDO sub-index
        for sm_mapping_sub_index in sm_range {
            let sm_type = self
                .read_sdo::<u8>(SM_TYPE_ADDRESS, SubIndex::Index(sm_mapping_sub_index))
                .await
                .map(SyncManagerType::from_primitive)?;

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
                    sm_bit_len,
                    fmmu_index,
                    gobal_offset,
                    desired_sm_type,
                    &sm_config,
                )
                .await?;
            }

            total_bit_len += sm_bit_len;
        }

        Ok(PdiSegment {
            bit_len: total_bit_len.into(),
            bytes: start_offset.up_to(*gobal_offset),
        })
    }

    async fn write_fmmu_config(
        &self,
        sm_bit_len: u16,
        fmmu_index: usize,
        global_offset: &mut PdiOffset,
        desired_sm_type: SyncManagerType,
        sm_config: &SyncManagerChannel,
    ) -> Result<(), Error> {
        // Multiple SMs may use the same FMMU, so we'll read the existing config from the slave
        let mut fmmu_config = self
            .read::<[u8; 16]>(RegisterAddress::fmmu(fmmu_index as u8), "read FMMU config")
            .await
            .and_then(|raw| Fmmu::unpack(&raw).map_err(|_| Error::Internal))?;

        // We can use the enable flag as a sentinel for existing config because EtherCrab inits
        // FMMUs to all zeroes on startup.
        let fmmu_config = if fmmu_config.enable {
            fmmu_config.length_bytes += sm_config.length_bytes;

            fmmu_config
        } else {
            Fmmu {
                logical_start_address: global_offset.start_address,
                length_bytes: sm_config.length_bytes,
                // Mapping into PDI is byte-aligned until/if we support bit-oriented slaves
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
        *global_offset = global_offset.increment_byte_aligned(sm_bit_len);
        Ok(())
    }

    /// Configure PDOs from EEPROM
    async fn configure_pdos_eeprom(
        &self,
        sync_managers: &[SyncManager],
        pdos: &[Pdo],
        fmmu_sm_mappings: &[FmmuEx],
        fmmu_usage: &[FmmuUsage],
        direction: PdoDirection,
        offset: &mut PdiOffset,
    ) -> Result<PdiSegment, Error> {
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

            let sm_config = self
                .write_sm_config(sync_manager_index, sync_manager, (bit_len + 7) / 8)
                .await?;

            self.write_fmmu_config(
                bit_len,
                usize::from(fmmu_index),
                offset,
                sm_type,
                &sm_config,
            )
            .await?;
        }

        Ok(PdiSegment {
            bit_len: total_bit_len.into(),
            bytes: start_offset.up_to(*offset),
        })
    }
}

/// Methods common to all states.
impl<'a, S> SlaveRef<'a, S> {
    pub fn new(client: &'a Client<'a>, configured_address: u16, state: S) -> Self {
        Self {
            client,
            configured_address,
            state,
        }
    }

    pub(crate) fn timeouts(&self) -> Timeouts {
        self.client.timeouts
    }

    pub async fn state(&self) -> Result<SlaveState, Error> {
        let (state, _code) = self.status().await?;

        Ok(state)
    }

    /// Read a register.
    ///
    /// Note that while this method is marked safe, alterations to slave config or behaviour can
    /// break interactions with EtherCrab.
    pub async fn raw_read<T>(&self, register: RegisterAddress) -> Result<T, Error>
    where
        T: PduRead,
        <T as PduRead>::Error: Debug,
    {
        self.read_ignore_wkc(register).await?.wkc(1, "raw read")
    }

    /// Write a register.
    ///
    /// Note that while this method is marked safe, alterations to slave config or behaviour can
    /// break interactions with EtherCrab.
    pub async fn raw_write<T>(&self, register: impl Into<u16>, value: T) -> Result<T, Error>
    where
        T: PduData,
        <T as PduRead>::Error: Debug,
    {
        self.write_ignore_wkc(register, value)
            .await?
            .wkc(1, "raw write")
    }

    pub(crate) async fn read_ignore_wkc<T>(
        &self,
        register: RegisterAddress,
    ) -> Result<PduResponse<T>, Error>
    where
        T: PduRead,
        <T as PduRead>::Error: Debug,
    {
        self.client.fprd(self.configured_address, register).await
    }

    /// A wrapper around an FPWR service to this slave's configured address.
    pub(crate) async fn write_ignore_wkc<T>(
        &self,
        register: impl Into<u16>,
        value: T,
    ) -> Result<PduResponse<T>, Error>
    where
        T: PduData,
        <T as PduRead>::Error: Debug,
    {
        self.client
            .fpwr(self.configured_address, register, value)
            .await
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
        self.read_ignore_wkc(register).await?.wkc(1, context)
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
        self.write_ignore_wkc(register, value)
            .await?
            .wkc(1, context)
    }

    pub async fn wait_for_state(&self, desired_state: SlaveState) -> Result<(), Error> {
        crate::timer_factory::timeout(self.client.timeouts.state_transition, async {
            loop {
                let status = self
                    .read::<AlControl>(RegisterAddress::AlStatus, "Read AL status")
                    .await?;

                if status.state == desired_state {
                    break Ok(());
                }

                self.client.timeouts.loop_tick().await;
            }
        })
        .await
    }

    pub async fn request_slave_state_nowait(&self, desired_state: SlaveState) -> Result<(), Error> {
        debug!(
            "Set state {} for slave address {:#04x}",
            desired_state, self.configured_address
        );

        // Send state request
        let response = self
            .write(
                RegisterAddress::AlControl,
                AlControl::new(desired_state).pack().unwrap(),
                "AL control",
            )
            .await
            .and_then(|raw: [u8; 2]| AlControl::unpack(&raw).map_err(|_| Error::StateTransition))?;

        if response.error {
            let error: AlStatusCode = self.read(RegisterAddress::AlStatus, "AL status").await?;

            log::error!(
                "Error occurred transitioning slave {:#06x} to {:?}: {}",
                self.configured_address,
                desired_state,
                error,
            );

            return Err(Error::StateTransition);
        }

        Ok(())
    }

    pub async fn request_slave_state(&self, desired_state: SlaveState) -> Result<(), Error> {
        self.request_slave_state_nowait(desired_state).await?;

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

    pub async fn set_eeprom_mode(&self, mode: SiiOwner) -> Result<(), Error> {
        self.write::<u16>(RegisterAddress::SiiConfig, 2, "debug write")
            .await?;
        self.write::<u16>(RegisterAddress::SiiConfig, mode as u16, "debug write 2")
            .await?;

        Ok(())
    }
}

pub enum PdoDirection {
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
