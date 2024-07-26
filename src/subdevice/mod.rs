pub(crate) mod configuration;
mod dc;
mod eeprom;
pub mod pdi;
pub mod ports;
mod types;

use crate::{
    al_control::AlControl,
    al_status_code::AlStatusCode,
    client::Client,
    coe::{
        self, abort_code::CoeAbortCode, services::CoeServiceRequest, CoeCommand, SdoExpedited,
        SubIndex,
    },
    command::Command,
    dl_status::DlStatus,
    eeprom::{device_reader::DeviceEeprom, types::SiiOwner},
    error::{Error, MailboxError, PduError},
    fmt,
    mailbox::{MailboxHeader, MailboxType},
    pdu_loop::ReceivedPdu,
    register::{DcSupport, RegisterAddress, SupportFlags},
    subdevice::{ports::Ports, types::SubDeviceConfig},
    subdevice_state::SubDeviceState,
    timer_factory::IntoTimeout,
    WrappedRead, WrappedWrite,
};
use core::{
    any::type_name,
    fmt::{Debug, Write},
    ops::{Deref, DerefMut},
    sync::atomic::{AtomicU8, Ordering},
};
use ethercrab_wire::{
    EtherCrabWireRead, EtherCrabWireReadSized, EtherCrabWireReadWrite, EtherCrabWireSized,
    EtherCrabWireWrite,
};

pub use self::pdi::SubDevicePdi;
pub use self::types::IoRanges;
pub use self::types::SubDeviceIdentity;
use self::{eeprom::SubDeviceEeprom, types::Mailbox};
pub use dc::DcSync;

/// SubDevice device metadata. See [`SubDeviceRef`] for richer behaviour.
#[derive(Debug)]
// Gated by test feature so we can easily create test cases, but not expose a `Default`-ed
// `SubDevice` to the user as this is an invalid state.
#[cfg_attr(test, derive(Default))]
pub struct SubDevice {
    /// Configured station address.
    pub(crate) configured_address: u16,

    pub(crate) alias_address: u16,

    pub(crate) config: SubDeviceConfig,

    pub(crate) identity: SubDeviceIdentity,

    // NOTE: Default length in SOEM is 40 bytes
    pub(crate) name: heapless::String<64>,

    pub(crate) flags: SupportFlags,

    pub(crate) ports: Ports,

    /// Distributed Clock latch receive time.
    pub(crate) dc_receive_time: u64,

    /// The index of the SubDevice in the EtherCAT tree.
    pub(crate) index: u16,

    /// The index of the previous SubDevice in the EtherCAT tree.
    ///
    /// For the first SubDevice in the network, this will always be `None`.
    pub(crate) parent_index: Option<u16>,

    /// Propagation delay in nanoseconds.
    ///
    /// `u32::MAX` gives a maximum propagation delay of ~4.2 seconds for the last SubDevice in the
    /// network.
    pub(crate) propagation_delay: u32,

    /// The 1-7 cyclic counter used when working with mailbox requests.
    pub(crate) mailbox_counter: AtomicU8,

    /// DC config.
    pub(crate) dc_sync: DcSync,
}

// Only required for tests, also doesn't make much sense - consumers of EtherCrab should be
// comparing e.g. `subdevice.identity()`, names, configured address or something other than the whole
// struct.
#[cfg(test)]
impl PartialEq for SubDevice {
    fn eq(&self, other: &Self) -> bool {
        self.configured_address == other.configured_address
            && self.alias_address == other.alias_address
            && self.config == other.config
            && self.identity == other.identity
            && self.name == other.name
            && self.flags == other.flags
            && self.ports == other.ports
            && self.dc_receive_time == other.dc_receive_time
            && self.index == other.index
            && self.parent_index == other.parent_index
            && self.propagation_delay == other.propagation_delay
            && self.dc_sync == other.dc_sync
        // NOTE: No mailbox_counter
    }
}

// SubDevices shouldn't really be clonable (IMO), but the tests need them to be, so this impl is
// feature gated.
#[cfg(test)]
impl Clone for SubDevice {
    fn clone(&self) -> Self {
        Self {
            configured_address: self.configured_address,
            alias_address: self.alias_address,
            config: self.config.clone(),
            identity: self.identity,
            name: self.name.clone(),
            flags: self.flags.clone(),
            ports: self.ports,
            dc_receive_time: self.dc_receive_time,
            index: self.index,
            parent_index: self.parent_index,
            propagation_delay: self.propagation_delay,
            dc_sync: self.dc_sync,
            mailbox_counter: AtomicU8::new(self.mailbox_counter.load(Ordering::Acquire)),
        }
    }
}

impl SubDevice {
    /// Create a SubDevice instance using the given configured address.
    ///
    /// This method reads the SubDevices's name and other identifying information, but does not
    /// configure it.
    pub(crate) async fn new<'sto>(
        client: &'sto Client<'sto>,
        index: u16,
        configured_address: u16,
    ) -> Result<Self, Error> {
        let subdevice_ref = SubDeviceRef::new(client, configured_address, ());

        fmt::debug!(
            "Waiting for SubDevice {:#06x} to enter {}",
            configured_address,
            SubDeviceState::Init
        );

        subdevice_ref.wait_for_state(SubDeviceState::Init).await?;

        // Make sure master has access to SubDevice EEPROM
        subdevice_ref.set_eeprom_mode(SiiOwner::Master).await?;

        let identity = subdevice_ref.eeprom().identity().await?;

        let name = subdevice_ref
            .eeprom()
            .device_name()
            .await?
            .unwrap_or_else(|| {
                let mut s = heapless::String::new();

                fmt::unwrap!(write!(
                    s,
                    "manu. {:#010x}, device {:#010x}, serial {:#010x}",
                    identity.vendor_id, identity.product_id, identity.serial
                )
                .map_err(|_| ()));

                s
            });

        let flags = subdevice_ref
            .read(RegisterAddress::SupportFlags)
            .receive::<SupportFlags>(client)
            .await?;

        let alias_address = subdevice_ref
            .read(RegisterAddress::ConfiguredStationAlias)
            .receive::<u16>(client)
            .await?;

        let ports = subdevice_ref
            .read(RegisterAddress::DlStatus)
            .receive::<DlStatus>(client)
            .await
            .map(|dl_status| {
                // NOTE: dc_receive_times are populated during DC initialisation
                // Ports in EtherCAT order 0 -> 3 -> 1 -> 2
                Ports::new(
                    dl_status.link_port0,
                    dl_status.link_port3,
                    dl_status.link_port1,
                    dl_status.link_port2,
                )
            })?;

        fmt::debug!(
            "SubDevice {:#06x} name {} {}, {}, {}, alias address {:#06x}",
            configured_address,
            name,
            identity,
            flags,
            ports,
            alias_address
        );

        Ok(Self {
            configured_address,
            alias_address,
            config: SubDeviceConfig::default(),
            index,
            parent_index: None,
            propagation_delay: 0,
            dc_receive_time: 0,
            identity,
            name,
            flags,
            ports,
            dc_sync: DcSync::Disabled,
            // 0 is a reserved value, so we initialise the cycle at 1. The cycle repeats 1 - 7.
            mailbox_counter: AtomicU8::new(1),
        })
    }

    /// Get the SubDevice's human readable name.
    pub fn name(&self) -> &str {
        self.name.as_str()
    }

    /// Get additional identifying details for the SubDevice.
    pub fn identity(&self) -> SubDeviceIdentity {
        self.identity
    }

    /// Get the configured station address of the SubDevice.
    pub fn configured_address(&self) -> u16 {
        self.configured_address
    }

    /// Get alias address for the SubDevice.
    pub fn alias_address(&self) -> u16 {
        self.alias_address
    }

    /// Get the network propagation delay of this device in nanoseconds.
    ///
    /// Note that before [`Client::init`](crate::client::Client::init) is called, this method will
    /// always return `0`.
    pub fn propagation_delay(&self) -> u32 {
        self.propagation_delay
    }

    /// Distributed Clock (DC) support.
    pub fn dc_support(&self) -> DcSupport {
        self.flags.dc_support()
    }

    pub(crate) fn io_segments(&self) -> &IoRanges {
        &self.config.io
    }

    /// Check if the current SubDevice is a child of `parent`.
    ///
    /// A SubDevice is a child of a parent if it is connected to an intermediate port of the
    /// parent device, i.e. is not connected to the last open port. In the latter case, this is a
    /// "downstream" device.
    ///
    /// # Examples
    ///
    /// An EK1100 (parent) with an EL2004 module connected (child) as well as another EK1914 coupler
    /// (downstream) connected has one child: the EL2004.
    pub(crate) fn is_child_of(&self, parent: &SubDevice) -> bool {
        // Only forks or crosses in the network can have child devices. Passthroughs only have
        // downstream devices.
        let parent_is_fork = parent.ports.topology().is_junction();

        let parent_port = parent.ports.port_assigned_to(self);

        // Children in a fork must be connected to intermediate ports
        let child_attached_to_last_parent_port =
            parent_port.is_some_and(|child_port| parent.ports.is_last_port(child_port));

        parent_is_fork && !child_attached_to_last_parent_port
    }
}

/// A wrapper around a [`SubDevice`] and additional state for richer behaviour.
///
/// For example, a `SubDeviceRef<SubDevicePdi>` is returned by
/// [`SubDeviceGroup`](crate::subdevice_group::SubDeviceGroup) methods to allow the reading and
/// writing of a SubDevice's process data.
#[derive(Debug)]
pub struct SubDeviceRef<'a, S> {
    pub(crate) client: &'a Client<'a>,
    pub(crate) configured_address: u16,
    state: S,
}

impl<'a> Clone for SubDeviceRef<'a, ()> {
    fn clone(&self) -> Self {
        Self {
            client: self.client,
            configured_address: self.configured_address,
            state: (),
        }
    }
}

impl<'a, S> SubDeviceRef<'a, S>
where
    S: DerefMut<Target = SubDevice>,
{
    /// Set DC sync configuration for this SubDevice.
    ///
    /// Note that this will not configure the SubDevice itself, but sets the configuration to be
    /// used by [`SubDeviceGroup::configure_dc_sync`](crate::SubDeviceGroup::configure_dc_sync).
    pub fn set_dc_sync(&mut self, dc_sync: DcSync) {
        self.state.dc_sync = dc_sync;
    }
}

impl<'a, S> SubDeviceRef<'a, S>
where
    S: Deref<Target = SubDevice>,
{
    /// Get the human readable name of the SubDevice.
    pub fn name(&self) -> &str {
        self.state.name.as_str()
    }

    /// Get additional identifying details for the SubDevice.
    pub fn identity(&self) -> SubDeviceIdentity {
        self.state.identity
    }

    /// Get alias address for the SubDevice.
    pub fn alias_address(&self) -> u16 {
        self.state.alias_address
    }

    /// Get the network propagation delay of this device in nanoseconds.
    ///
    /// Note that before [`Client::init`](crate::client::Client::init) is called, this method will
    /// always return `0`.
    pub fn propagation_delay(&self) -> u32 {
        self.state.propagation_delay
    }

    /// Distributed Clock (DC) support.
    pub fn dc_support(&self) -> DcSupport {
        self.state.flags.dc_support()
    }

    pub(crate) fn dc_sync(&self) -> DcSync {
        self.state.dc_sync
    }

    /// Return the current cyclic mailbox counter value, from 0-7.
    ///
    /// Calling this method internally increments the counter, so subequent calls will produce a new
    /// value.
    fn mailbox_counter(&self) -> u8 {
        fmt::unwrap!(self.state.mailbox_counter.fetch_update(
            Ordering::Release,
            Ordering::Acquire,
            |n| {
                if n >= 7 {
                    Some(1)
                } else {
                    Some(n + 1)
                }
            }
        ))
    }

    /// Get CoE read/write mailboxes.
    async fn coe_mailboxes(&self) -> Result<(Mailbox, Mailbox), Error> {
        let write_mailbox = self
            .state
            .config
            .mailbox
            .write
            .ok_or(Error::Mailbox(MailboxError::NoMailbox))
            .map_err(|e| {
                fmt::error!("No write (SubDevice IN) mailbox found but one is required");
                e
            })?;
        let read_mailbox = self
            .state
            .config
            .mailbox
            .read
            .ok_or(Error::Mailbox(MailboxError::NoMailbox))
            .map_err(|e| {
                fmt::error!("No read (SubDevice OUT) mailbox found but one is required");
                e
            })?;

        let mailbox_read_sm_status =
            RegisterAddress::sync_manager_status(read_mailbox.sync_manager);
        let mailbox_write_sm_status =
            RegisterAddress::sync_manager_status(write_mailbox.sync_manager);

        // Ensure SubDevice OUT (master IN) mailbox is empty
        {
            let sm_status = self
                .read(mailbox_read_sm_status)
                .receive::<crate::sync_manager_channel::Status>(self.client)
                .await?;

            // If flag is set, read entire mailbox to clear it
            if sm_status.mailbox_full {
                fmt::debug!(
                    "SubDevice {:#06x} OUT mailbox not empty. Clearing.",
                    self.configured_address()
                );

                self.read(read_mailbox.address)
                    .ignore_wkc()
                    .receive_slice(self.client, read_mailbox.len)
                    .await?;
            }
        }

        // Wait for SubDevice IN mailbox to be available to receive data from master
        async {
            loop {
                let sm_status = self
                    .read(mailbox_write_sm_status)
                    .receive::<crate::sync_manager_channel::Status>(self.client)
                    .await?;

                if !sm_status.mailbox_full {
                    break Ok(());
                }

                self.client.timeouts.loop_tick().await;
            }
        }
        .timeout(self.client.timeouts.mailbox_echo)
        .await
        .map_err(|e| {
            fmt::error!(
                "Mailbox IN ready error for SubDevice {:#06x}: {}",
                self.configured_address,
                e
            );

            e
        })?;

        Ok((read_mailbox, write_mailbox))
    }

    /// Wait for a mailbox response
    async fn coe_response(&self, read_mailbox: &Mailbox) -> Result<ReceivedPdu, Error> {
        let mailbox_read_sm = RegisterAddress::sync_manager_status(read_mailbox.sync_manager);

        // Wait for SubDevice OUT mailbox to be ready
        async {
            loop {
                let sm_status = self
                    .read(mailbox_read_sm)
                    .receive::<crate::sync_manager_channel::Status>(self.client)
                    .await?;

                if sm_status.mailbox_full {
                    break Ok(());
                }

                self.client.timeouts.loop_tick().await;
            }
        }
        .timeout(self.client.timeouts.mailbox_echo)
        .await
        .map_err(|e| {
            fmt::error!(
                "Response mailbox IN error for SubDevice {:#06x}: {}",
                self.configured_address,
                e
            );

            e
        })?;

        // Read acknowledgement from SubDevice OUT mailbox
        let response = self
            .read(read_mailbox.address)
            .receive_slice(self.client, read_mailbox.len)
            .await?;

        // TODO: Retries. Refer to SOEM's `ecx_mbxreceive` for inspiration

        Ok(response)
    }

    /// Send a mailbox request, wait for response mailbox to be ready, read response from mailbox
    /// and return as a slice.
    async fn send_coe_service<R>(&'a self, request: R) -> Result<(R, ReceivedPdu), Error>
    where
        R: CoeServiceRequest + Debug,
    {
        let (read_mailbox, write_mailbox) = self.coe_mailboxes().await?;

        let counter = request.counter();

        // Send data to SubDevice IN mailbox
        self.write(write_mailbox.address)
            .with_len(write_mailbox.len)
            .send(self.client, &request.pack().as_ref())
            .await?;

        let mut response = self.coe_response(&read_mailbox).await?;

        /// A super generalised version of the various header shapes for responses, extracting only
        /// what we need in this method.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, ethercrab_wire::EtherCrabWireRead)]
        #[wire(bytes = 12)]
        struct HeadersRaw {
            #[wire(bytes = 8)]
            header: MailboxHeader,

            #[wire(pre_skip = 5, bits = 3)]
            command: CoeCommand,

            // 9 bytes up to here

            // SAFETY: These fields will be garbage (but not invalid) if the response is NOT an
            // abort transfer request. Use with caution!
            #[wire(bytes = 2)]
            address: u16,
            #[wire(bytes = 1)]
            sub_index: u8,
        }

        let headers = HeadersRaw::unpack_from_slice(&response)?;

        if headers.command == CoeCommand::Abort {
            let code = CoeAbortCode::Incompatible;

            fmt::error!(
                "Mailbox error for SubDevice {:#06x} (supports complete access: {}): {}",
                self.configured_address,
                self.state.config.mailbox.complete_access,
                code
            );

            Err(Error::Mailbox(MailboxError::Aborted {
                code,
                address: headers.address,
                sub_index: headers.sub_index,
            }))
        }
        // Validate that the mailbox response is to the request we just sent
        else if headers.header.mailbox_type != MailboxType::Coe
            || headers.header.counter != counter
        {
            fmt::error!(
                "Invalid SDO response. Type: {:?} (expected {:?}), counter {} (expected {})",
                headers.header.mailbox_type,
                MailboxType::Coe,
                headers.header.counter,
                counter
            );

            Err(Error::Mailbox(MailboxError::SdoResponseInvalid {
                address: headers.address,
                sub_index: headers.sub_index,
            }))
        } else {
            let headers = R::unpack_from_slice(&response)?;

            response.trim_front(HeadersRaw::PACKED_LEN);

            Ok((headers, response))
        }
    }

    /// Write a value to the given SDO index (address) and sub-index.
    ///
    /// Note that this method currently only supports expedited SDO downloads (4 bytes maximum).
    pub async fn sdo_write<T>(
        &self,
        index: u16,
        sub_index: impl Into<SubIndex>,
        value: T,
    ) -> Result<(), Error>
    where
        T: EtherCrabWireWrite,
    {
        let sub_index = sub_index.into();

        let counter = self.mailbox_counter();

        if value.packed_len() > 4 {
            fmt::error!("Only 4 byte SDO writes or smaller are supported currently.");

            // TODO: Normal SDO download. Only expedited requests for now
            return Err(Error::Internal);
        }

        let mut buf = [0u8; 4];

        value.pack_to_slice(&mut buf)?;

        let request =
            coe::services::download(counter, index, sub_index, buf, value.packed_len() as u8);

        fmt::trace!("CoE download");

        let (_response, _data) = self.send_coe_service(request).await?;

        // TODO: Validate reply?

        Ok(())
    }

    pub(crate) async fn sdo_read_expedited<T>(
        &self,
        index: u16,
        sub_index: impl Into<SubIndex>,
    ) -> Result<T, Error>
    where
        T: SdoExpedited,
    {
        debug_assert!(
            T::PACKED_LEN <= 4,
            "expedited transfers are up to 4 bytes long, this T is {}",
            T::PACKED_LEN
        );

        let sub_index = sub_index.into();

        let request = coe::services::upload(self.mailbox_counter(), index, sub_index);

        fmt::trace!("CoE upload {:#06x} {:?}", index, sub_index);

        let (headers, response) = self.send_coe_service(request).await?;
        let data: &[u8] = &response;

        // Expedited transfers where the data is 4 bytes or less long, denoted in the SDO header
        // size value.
        if headers.sdo_header.expedited_transfer {
            let data_len = 4usize.saturating_sub(usize::from(headers.sdo_header.size));

            Ok(T::unpack_from_slice(
                data.get(0..data_len).ok_or(Error::Internal)?,
            )?)
        } else {
            Err(Error::Internal)
        }
    }

    /// Read a value from an SDO (Service Data Object) from the given index (address) and sub-index.
    pub async fn sdo_read<T>(&self, index: u16, sub_index: impl Into<SubIndex>) -> Result<T, Error>
    where
        T: EtherCrabWireReadSized,
    {
        let sub_index = sub_index.into();

        let mut storage = T::buffer();
        let buf = storage.as_mut();

        let request = coe::services::upload(self.mailbox_counter(), index, sub_index);

        fmt::trace!("CoE upload {:#06x} {:?}", index, sub_index);

        let (headers, response) = self.send_coe_service(request).await?;
        let data: &[u8] = &response;

        // Expedited transfers where the data is 4 bytes or less long, denoted in the SDO header
        // size value.
        let response_payload = if headers.sdo_header.expedited_transfer {
            let data_len = 4usize.saturating_sub(usize::from(headers.sdo_header.size));

            data.get(0..data_len).ok_or(Error::Internal)?
        }
        // Data is either a normal upload or a segmented upload
        else {
            let data_length = headers.header.length.saturating_sub(0x0a);

            let complete_size = u32::unpack_from_slice(data)?;
            let data = data.get(u32::PACKED_LEN..).ok_or(Error::Internal)?;

            // The provided buffer isn't long enough to contain all mailbox data.
            if complete_size > buf.len() as u32 {
                return Err(Error::Mailbox(MailboxError::TooLong {
                    address: headers.sdo_header.index,
                    sub_index: headers.sdo_header.sub_index,
                }));
            }

            // If it's a normal upload, the response payload is returned in the initial mailbox read
            if complete_size <= u32::from(data_length) {
                data.get(0..usize::from(data_length))
                    .ok_or(Error::Internal)?
            }
            // If it's a segmented upload, we must make subsequent requests to load all segment data
            // from the read mailbox.
            else {
                let mut toggle = false;
                let mut total_len = 0usize;

                loop {
                    let request = coe::services::upload_segmented(self.mailbox_counter(), toggle);

                    fmt::trace!("CoE upload segmented");

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

                    let data = data.get(0..chunk_len).ok_or(Error::Internal)?;

                    buf.get_mut(total_len..(total_len + chunk_len))
                        .ok_or(Error::Internal)?
                        .copy_from_slice(data);

                    total_len += chunk_len;

                    if headers.sdo_header.is_last_segment {
                        break;
                    }

                    toggle = !toggle;
                }

                buf.get(0..total_len).ok_or(Error::Internal)?
            }
        };

        T::unpack_from_slice(response_payload).map_err(|_| {
            fmt::error!(
                "SDO expedited data decode T: {} (len {}) data {:?} (len {})",
                type_name::<T>(),
                T::PACKED_LEN,
                response_payload,
                response_payload.len()
            );

            Error::Pdu(PduError::Decode)
        })
    }
}

// General impl with no bounds
impl<'a, S> SubDeviceRef<'a, S> {
    pub(crate) fn new(client: &'a Client<'a>, configured_address: u16, state: S) -> Self {
        Self {
            client,
            configured_address,
            state,
        }
    }

    /// Get the configured station address of the SubDevice.
    pub fn configured_address(&self) -> u16 {
        self.configured_address
    }

    /// Get the sub device status.
    pub(crate) async fn state(&self) -> Result<SubDeviceState, Error> {
        match self
            .read(RegisterAddress::AlStatus)
            .receive::<AlControl>(self.client)
            .await
            .and_then(|ctl| {
                if ctl.error {
                    Err(Error::SubDevice(AlStatusCode::Unknown(0)))
                } else {
                    Ok(ctl.state)
                }
            }) {
            Ok(state) => Ok(state),
            Err(e) => match e {
                Error::SubDevice(AlStatusCode::Unknown(0)) => {
                    let code = self
                        .read(RegisterAddress::AlStatusCode)
                        .receive::<AlStatusCode>(self.client)
                        .await
                        .unwrap_or(AlStatusCode::Unknown(0));

                    Err(Error::SubDevice(code))
                }
                e => Err(e),
            },
        }
    }

    /// Get the EtherCAT state machine state of the sub device.
    pub async fn status(&self) -> Result<(SubDeviceState, AlStatusCode), Error> {
        let code = self
            .read(RegisterAddress::AlStatusCode)
            .receive::<AlStatusCode>(self.client);

        futures_lite::future::try_zip(self.state(), code).await
    }

    fn eeprom(&self) -> SubDeviceEeprom<DeviceEeprom> {
        SubDeviceEeprom::new(DeviceEeprom::new(self.client, self.configured_address))
    }

    /// Read a register.
    ///
    /// Note that while this method is marked safe, raw alterations to SubDevice config or behaviour can
    /// break higher level interactions with EtherCrab.
    pub async fn register_read<T>(&self, register: impl Into<u16>) -> Result<T, Error>
    where
        T: EtherCrabWireReadSized,
    {
        self.read(register.into()).receive(self.client).await
    }

    /// Write a register.
    ///
    /// Note that while this method is marked safe, raw alterations to SubDevice config or behaviour can
    /// break higher level interactions with EtherCrab.
    pub async fn register_write<T>(&self, register: impl Into<u16>, value: T) -> Result<T, Error>
    where
        T: EtherCrabWireReadWrite,
    {
        self.write(register.into())
            .send_receive(self.client, value)
            .await
    }

    pub(crate) async fn wait_for_state(&self, desired_state: SubDeviceState) -> Result<(), Error> {
        async {
            loop {
                let status = self
                    .read(RegisterAddress::AlStatus)
                    .ignore_wkc()
                    .receive::<AlControl>(self.client)
                    .await?;

                if status.state == desired_state {
                    break Ok(());
                }

                self.client.timeouts.loop_tick().await;
            }
        }
        .timeout(self.client.timeouts.state_transition)
        .await
    }

    pub(crate) fn write(&self, register: impl Into<u16>) -> WrappedWrite {
        Command::fpwr(self.configured_address, register.into())
    }

    pub(crate) fn read(&self, register: impl Into<u16>) -> WrappedRead {
        Command::fprd(self.configured_address, register.into())
    }

    pub(crate) async fn request_subdevice_state_nowait(
        &self,
        desired_state: SubDeviceState,
    ) -> Result<(), Error> {
        fmt::debug!(
            "Set state {} for SubDevice address {:#04x}",
            desired_state,
            self.configured_address
        );

        // Send state request
        let response = self
            .write(RegisterAddress::AlControl)
            .send_receive::<AlControl>(self.client, AlControl::new(desired_state))
            .await?;

        if response.error {
            let error = self
                .read(RegisterAddress::AlStatus)
                .receive::<AlStatusCode>(self.client)
                .await?;

            fmt::error!(
                "Error occurred transitioning SubDevice {:#06x} to {:?}: {}",
                self.configured_address,
                desired_state,
                error,
            );

            return Err(Error::StateTransition);
        }

        Ok(())
    }

    pub(crate) async fn request_subdevice_state(
        &self,
        desired_state: SubDeviceState,
    ) -> Result<(), Error> {
        self.request_subdevice_state_nowait(desired_state).await?;

        self.wait_for_state(desired_state).await
    }

    pub(crate) async fn set_eeprom_mode(&self, mode: SiiOwner) -> Result<(), Error> {
        // ETG1000.4 Table 48 – SubDevice information interface access
        // A value of 2 sets owner to Master (not PDI) and cancels access
        self.write(RegisterAddress::SiiConfig)
            .send(self.client, 2u16)
            .await?;

        self.write(RegisterAddress::SiiConfig)
            .send(self.client, mode)
            .await?;

        Ok(())
    }
}
