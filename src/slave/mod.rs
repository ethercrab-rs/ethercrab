pub(crate) mod configuration;
mod eeprom;
pub mod pdi;
pub mod ports;
mod types;

use self::types::{SlaveConfig, SlaveIdentity};
use crate::{
    al_control::AlControl,
    al_status_code::AlStatusCode,
    client::Client,
    coe::SubIndex,
    coe::{
        self,
        abort_code::AbortCode,
        services::{CoeServiceRequest, CoeServiceResponse},
    },
    command::Command,
    dl_status::DlStatus,
    eeprom::types::SiiOwner,
    error::{Error, MailboxError, PduError},
    mailbox::MailboxType,
    pdu_data::{PduData, PduRead},
    pdu_loop::{CheckWorkingCounter, PduResponse, RxFrameDataBuf},
    register::RegisterAddress,
    register::SupportFlags,
    slave::ports::{Port, Ports},
    slave_state::SlaveState,
    sync_manager_channel::SyncManagerChannel,
    Timeouts,
};
use core::{
    any::type_name,
    borrow::Borrow,
    fmt::{Debug, Write},
};
use nom::{bytes::complete::take, number::complete::le_u32};
use packed_struct::{PackedStruct, PackedStructInfo, PackedStructSlice};

pub use self::pdi::SlavePdi;
pub use self::types::IoRanges;

/// Basic slave data.
///
/// See [`SlaveRef`] for richer behaviour.
#[derive(Debug, Clone, PartialEq)]
// Gated by test feature so we can easily create test cases, but not expose a `Default`-ed `Slave`
// to the user as this is an invalid state.
#[cfg_attr(test, derive(Default))]
pub struct Slave {
    /// Configured station address.
    pub(crate) configured_address: u16,

    pub(crate) config: SlaveConfig,

    pub(crate) identity: SlaveIdentity,

    // NOTE: Default length in SOEM is 40 bytes
    pub(crate) name: heapless::String<64>,

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
    }

    /// Get the slave device's human readable name.
    pub fn name(&self) -> &str {
        self.name.as_str()
    }

    /// Get additional identifying details for the slave device.
    pub fn identity(&self) -> SlaveIdentity {
        self.identity
    }

    pub(crate) fn io_segments(&self) -> &IoRanges {
        &self.config.io
    }
}

/// A slave device with additional metadata and methods.
#[derive(Debug)]
pub struct SlaveRef<'a, S> {
    client: &'a Client<'a>,
    configured_address: u16,
    state: S,
}

impl<'a, S> SlaveRef<'a, S>
where
    S: Borrow<Slave>,
{
    /// Get the human readable name of the slave device.
    pub fn name(&self) -> &str {
        self.state.borrow().name.as_str()
    }

    /// Get the configured station address of the slave device.
    pub fn configured_address(&self) -> u16 {
        self.configured_address
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

    /// Write a value to the given SDO index (address) and sub-index.
    pub async fn write_sdo<T>(
        &self,
        index: u16,
        sub_index: impl Into<SubIndex>,
        value: T,
    ) -> Result<(), Error>
    where
        T: PduData,
        <T as PduRead>::Error: Debug,
    {
        let sub_index = sub_index.into();

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
        sub_index: impl Into<SubIndex>,
        buf: &'buf mut [u8],
    ) -> Result<&'buf [u8], Error> {
        let sub_index = sub_index.into();

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

    /// Read a value from an SDO (Service Data Object) from the given index (address) and sub-index.
    pub async fn read_sdo<T>(&self, index: u16, sub_index: impl Into<SubIndex>) -> Result<T, Error>
    where
        T: PduData,
        <T as PduRead>::Error: Debug,
    {
        let sub_index = sub_index.into();

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

impl<'a, S> SlaveRef<'a, S> {
    pub(crate) fn new(client: &'a Client<'a>, configured_address: u16, state: S) -> Self {
        Self {
            client,
            configured_address,
            state,
        }
    }

    pub(crate) fn timeouts(&self) -> Timeouts {
        self.client.timeouts
    }

    /// Get the EtherCAT state machine state of the slave.
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

    /// Read a register.
    ///
    /// Note that while this method is marked safe, alterations to slave config or behaviour can
    /// break interactions with EtherCrab.
    pub async fn read_raw<T>(&self, register: RegisterAddress) -> Result<T, Error>
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
    pub async fn write_raw<T>(&self, register: impl Into<u16>, value: T) -> Result<T, Error>
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

    pub(crate) async fn wait_for_state(&self, desired_state: SlaveState) -> Result<(), Error> {
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

    pub(crate) async fn request_slave_state_nowait(
        &self,
        desired_state: SlaveState,
    ) -> Result<(), Error> {
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

    pub(crate) async fn request_slave_state(&self, desired_state: SlaveState) -> Result<(), Error> {
        self.request_slave_state_nowait(desired_state).await?;

        self.wait_for_state(desired_state).await
    }

    pub(crate) async fn set_eeprom_mode(&self, mode: SiiOwner) -> Result<(), Error> {
        self.write::<u16>(RegisterAddress::SiiConfig, 2, "debug write")
            .await?;
        self.write::<u16>(RegisterAddress::SiiConfig, mode as u16, "debug write 2")
            .await?;

        Ok(())
    }
}
