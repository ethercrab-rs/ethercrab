pub(crate) mod configuration;
mod dc;
mod eeprom;
pub mod pdi;
pub mod ports;
mod types;

use crate::{
    WrappedRead, WrappedWrite,
    al_control::AlControl,
    al_status_code::AlStatusCode,
    command::Command,
    dl_status::DlStatus,
    eeprom::{device_provider::DeviceEeprom, types::SiiOwner},
    error::{Error, IgnoreNoCategory},
    fmt,
    mailbox::coe::{self, Coe, SdoExpeditedPayload, SubIndex},
    maindevice::MainDevice,
    register::{DcSupport, RegisterAddress, SupportFlags},
    subdevice::{ports::Ports, types::SubDeviceConfig},
    subdevice_state::SubDeviceState,
    timer_factory::IntoTimeout,
};
use core::{
    fmt::{Debug, Write},
    ops::{Deref, DerefMut},
    sync::atomic::{AtomicU8, Ordering},
};
use embedded_io_async::{Read, Write as EioWrite};
use ethercrab_wire::{
    EtherCrabWireReadSized, EtherCrabWireReadWrite, EtherCrabWireWrite, EtherCrabWireWriteSized,
};

use self::eeprom::SubDeviceEeprom;
pub use self::pdi::SubDevicePdi;
pub use self::types::IoRanges;
pub use self::types::Mailbox;
pub use self::types::SubDeviceIdentity;
pub use coe::services::{ObjectDescriptionListQuery, ObjectDescriptionListQueryCounts};
pub use dc::DcSync;

/// SubDevice device metadata. See [`SubDeviceRef`] for richer behaviour.
#[doc(alias = "Slave")]
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

    // pub(crate) flags: SupportFlags,
    pub(crate) ports: Ports,

    pub(crate) dc_support: DcSupport,

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

    pub(crate) oversampling_config: &'static [(u16, u8)],
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
            && self.dc_support == other.dc_support
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
            dc_support: self.dc_support,
            ports: self.ports,
            dc_receive_time: self.dc_receive_time,
            index: self.index,
            parent_index: self.parent_index,
            propagation_delay: self.propagation_delay,
            dc_sync: self.dc_sync,
            mailbox_counter: AtomicU8::new(self.mailbox_counter.load(Ordering::Acquire)),
            oversampling_config: &[],
        }
    }
}

impl SubDevice {
    /// Create a SubDevice instance using the given configured address.
    ///
    /// This method reads the SubDevices's name and other identifying information, but does not
    /// configure it.
    pub(crate) async fn new<'sto>(
        maindevice: &'sto MainDevice<'sto>,
        index: u16,
        configured_address: u16,
    ) -> Result<Self, Error> {
        let subdevice_ref = SubDeviceRef::new(maindevice, configured_address, ());

        fmt::debug!(
            "Waiting for SubDevice {:#06x} to enter {}",
            configured_address,
            SubDeviceState::Init
        );

        subdevice_ref.wait_for_state(SubDeviceState::Init).await?;

        // Make sure master has access to SubDevice EEPROM
        subdevice_ref.set_eeprom_mode(SiiOwner::Master).await?;

        let eeprom = subdevice_ref.eeprom();

        let identity = eeprom.identity().await?;

        let name = eeprom.device_name().await?.unwrap_or_else(|| {
            let mut s = heapless::String::new();

            fmt::unwrap!(
                write!(
                    s,
                    "manu. {:#010x}, device {:#010x}, serial {:#010x}",
                    identity.vendor_id, identity.product_id, identity.serial
                )
                .map_err(|_| ())
            );

            s
        });

        let flags = subdevice_ref
            .read(RegisterAddress::SupportFlags)
            .receive::<SupportFlags>(maindevice)
            .await?;

        fmt::debug!("--> Support flags {:?}", flags);

        let alias_address = subdevice_ref
            .read(RegisterAddress::ConfiguredStationAlias)
            .receive::<u16>(maindevice)
            .await?;

        let ports = subdevice_ref
            .read(RegisterAddress::DlStatus)
            .receive::<DlStatus>(maindevice)
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
            dc_support: flags.dc_support(),
            ports,
            dc_sync: DcSync::Disabled,
            // 0 is a reserved value, so we initialise the cycle at 1. The cycle repeats 1 - 7.
            mailbox_counter: AtomicU8::new(1),
            oversampling_config: &[],
        })
    }

    /// Set oversampling values for various PDOs.
    ///
    /// This is a temporary(ish) solution to configure oversampling until a better one is found to
    /// configure this from ESI files, etc.
    pub fn set_oversampling(&mut self, oversampling_config: &'static [(u16, u8)]) {
        self.oversampling_config = oversampling_config
    }

    /// Get the SubDevice's human readable short name.
    ///
    /// To get a longer, more descriptive name, use [`SubDevice::description`].
    pub fn name(&self) -> &str {
        self.name.as_str()
    }

    /// Get the long name of the SubDevice.
    ///
    /// Using the EK1100 as an example, [`SubDevice::name`](fn@crate::SubDevice::name) will return
    /// `"EK1100"` whereas this method will return `"EK1100 EtherCAT-Koppler (2A E-Bus)"`.
    ///
    /// In the case that a SubDevice does not have a description, this method will return
    /// `Ok(None)`.
    pub async fn description(
        &self,
        maindevice: &MainDevice<'_>,
    ) -> Result<Option<heapless::String<128>>, Error> {
        let subdevice_ref = SubDeviceRef::new(maindevice, self.configured_address, ());

        Ok(subdevice_ref
            .eeprom()
            .device_description()
            .await
            .ignore_no_category()?
            .flatten())
    }

    /// Get the SubDevice's EEPROM size in bytes.
    pub async fn eeprom_size(&self, maindevice: &MainDevice<'_>) -> Result<usize, Error> {
        let subdevice_ref = SubDeviceRef::new(maindevice, self.configured_address, ());

        subdevice_ref.eeprom().size().await
    }

    /// Read raw bytes from the SubDevice's EEPROM, starting at the given **word** address.
    ///
    /// **The given start address is in words NOT bytes. To address the EEPROM using a byte address,
    /// divide the given byte address by two.**
    ///
    /// To read individual typed values including fixed size chunks of `[u8; N]`, see
    /// [`eeprom_read`](SubDevice::eeprom_read).
    pub async fn eeprom_read_raw(
        &self,
        maindevice: &MainDevice<'_>,
        start_word: u16,
        buf: &mut [u8],
    ) -> Result<usize, Error> {
        let subdevice_ref = SubDeviceRef::new(maindevice, self.configured_address, ());

        let mut reader = subdevice_ref
            .eeprom()
            .start_at(start_word, buf.len() as u16);

        reader.read(buf).await
    }

    /// Read a value from the SubDevice's EEPROM at the given **word** address.
    ///
    /// **The given start address is in words NOT bytes. To address the EEPROM using a byte address,
    /// divide the given byte address by two.**
    ///
    /// To read raw bytes, see [`eeprom_read_raw`](SubDevice::eeprom_read_raw).
    pub async fn eeprom_read<T>(
        &self,
        maindevice: &MainDevice<'_>,
        start_word: u16,
    ) -> Result<T, Error>
    where
        T: EtherCrabWireReadSized,
    {
        let subdevice_ref = SubDeviceRef::new(maindevice, self.configured_address, ());

        let mut reader = subdevice_ref
            .eeprom()
            .start_at(start_word, T::PACKED_LEN as u16);

        let mut buf = T::buffer();

        reader.read_exact(buf.as_mut()).await?;

        let result = T::unpack_from_slice(buf.as_ref())?;

        Ok(result)
    }

    /// Write a value to the SubDevice's EEPROM at the given **word** address.
    ///
    /// <div class="warning">
    ///
    /// **Warning:** This method is safe in the Rust sense, but can cause **EEPROM corruption** if
    /// mishandled. Be **very** careful when writing data to a SubDevice's EEPROM.
    ///
    /// </div>
    ///
    /// **The given start address is in words NOT bytes. To address the EEPROM using a byte address,
    /// divide the given byte address by two.**
    pub async fn eeprom_write_dangerously<T>(
        &self,
        maindevice: &MainDevice<'_>,
        start_word: u16,
        value: T,
    ) -> Result<(), Error>
    where
        T: EtherCrabWireWriteSized,
    {
        let subdevice_ref = SubDeviceRef::new(maindevice, self.configured_address, ());

        let mut writer = subdevice_ref
            .eeprom()
            .start_at(start_word, T::PACKED_LEN as u16);

        writer.write_all(value.pack().as_ref()).await?;

        Ok(())
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

    /// Set the alias address for the SubDevice and store it in EEPROM.
    ///
    /// The new alias address can be used within EtherCrab immediately, but a power cycle is
    /// recommended to properly refresh all state.
    pub async fn set_alias_address(
        &mut self,
        maindevice: &MainDevice<'_>,
        new_alias: u16,
    ) -> Result<(), Error> {
        let subdevice_ref = SubDeviceRef::new(maindevice, self.configured_address, ());

        subdevice_ref.eeprom().set_station_alias(new_alias).await?;

        self.alias_address = new_alias;

        Ok(())
    }

    /// Get the network propagation delay of this device in nanoseconds.
    ///
    /// Note that before [`MainDevice::init`](crate::MainDevice::init) is called, this method will
    /// always return `0`.
    pub fn propagation_delay(&self) -> u32 {
        self.propagation_delay
    }

    /// Distributed Clock (DC) support.
    pub fn dc_support(&self) -> DcSupport {
        self.dc_support
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

    /// Return the current cyclic mailbox counter value, from 0-7.
    ///
    /// Calling this method internally increments the counter, so subequent calls will produce a new
    /// value.
    pub(crate) fn mailbox_counter(&self) -> u8 {
        fmt::unwrap!(
            self.mailbox_counter
                .fetch_update(Ordering::Release, Ordering::Acquire, |n| {
                    if n >= 7 { Some(1) } else { Some(n + 1) }
                })
        )
    }
}

/// A wrapper around a [`SubDevice`] and additional state for richer behaviour.
///
/// For example, a `SubDeviceRef<SubDevicePdi>` is returned by
/// [`SubDeviceGroup`](crate::subdevice_group::SubDeviceGroup) methods to allow the reading and
/// writing of a SubDevice's process data.
#[derive(Debug)]
#[doc(alias = "SlaveRef")]
pub struct SubDeviceRef<'maindevice, S> {
    pub(crate) maindevice: &'maindevice MainDevice<'maindevice>,
    pub(crate) configured_address: u16,
    state: S,
}

impl Clone for SubDeviceRef<'_, ()> {
    fn clone(&self) -> Self {
        Self {
            maindevice: self.maindevice,
            configured_address: self.configured_address,
            state: (),
        }
    }
}

impl<S> DerefMut for SubDeviceRef<'_, S>
where
    S: DerefMut<Target = SubDevice>,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.state
    }
}

impl<S> SubDeviceRef<'_, S>
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

    /// Set the alias address for the SubDevice and store it in EEPROM.
    ///
    /// The new alias address can be used within EtherCrab immediately, but a power cycle is
    /// recommended to properly refresh all state.
    pub async fn set_alias_address(&mut self, new_alias: u16) -> Result<(), Error> {
        SubDevice::set_alias_address(&mut self.state, self.maindevice, new_alias).await
    }
}

impl<S> Deref for SubDeviceRef<'_, S>
where
    S: Deref<Target = SubDevice>,
{
    type Target = SubDevice;

    fn deref(&self) -> &Self::Target {
        &self.state
    }
}

impl<'maindevice, S> SubDeviceRef<'maindevice, S>
where
    S: Deref<Target = SubDevice>,
{
    /// Get the long name of the SubDevice.
    ///
    /// Using the EK1100 as an example, the [`name`](crate::SubDevice::name) method will return
    /// `"EK1100"` whereas this method will return `"EK1100 EtherCAT-Koppler (2A E-Bus)"`.
    ///
    /// In the case that a SubDevice does not have a description, this method will return
    /// `Ok(None)`.
    pub async fn description(&self) -> Result<Option<heapless::String<128>>, Error> {
        SubDevice::description(&self.state, self.maindevice).await
    }

    /// INTERNAL: Read address from EEPROM.
    ///
    /// Useful for testing. Please don't rely on this as a public API item.
    #[doc(hidden)]
    pub async fn read_alias_address_from_eeprom(
        &self,
        maindevice: &MainDevice<'_>,
    ) -> Result<u16, Error> {
        let subdevice_ref = SubDeviceRef::new(maindevice, self.configured_address, ());

        subdevice_ref.eeprom().station_alias().await
    }

    pub(crate) fn dc_sync(&self) -> DcSync {
        self.state.dc_sync
    }

    /// Read a value from an SDO (Service Data Object) from the given index (address) and sub-index.
    pub async fn sdo_read<T>(&self, index: u16, sub_index: impl Into<SubIndex>) -> Result<T, Error>
    where
        T: EtherCrabWireReadSized,
    {
        Coe::new(self).sdo_read(index, sub_index).await
    }

    pub(crate) async fn sdo_read_expedited<T>(
        &self,
        index: u16,
        sub_index: impl Into<SubIndex>,
    ) -> Result<T, Error>
    where
        T: SdoExpeditedPayload,
    {
        Coe::new(self).sdo_read_expedited(index, sub_index).await
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
        Coe::new(self).sdo_write(index, sub_index, value).await
    }

    /// Write multiple sub-indices of the given SDO.
    ///
    /// This is NOT a complete access write. This method is provided as sugar over individual calls
    /// to [`sdo_write`](SubDeviceRef::sdo_write) and handles setting the SDO length and sub-index
    /// automatically.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use ethercrab::{
    /// #     error::Error, MainDevice, MainDeviceConfig, PduStorage, Timeouts, std::ethercat_now
    /// # };
    /// # static PDU_STORAGE: PduStorage<8, 32> = PduStorage::new();
    /// # let (_tx, _rx, pdu_loop) = PDU_STORAGE.try_split().expect("can only split once");
    /// # let maindevice = MainDevice::new(pdu_loop, Timeouts::default(), MainDeviceConfig::default());
    /// # async {
    /// # let mut group = maindevice
    /// #     .init_single_group::<8, 8>(ethercat_now)
    /// #     .await
    /// #     .expect("Init");
    /// let subdevice = group.subdevice(&maindevice, 0).expect("No subdevice!");
    ///
    /// // This is equivalent to...
    /// subdevice.sdo_write_array(0x1c13, &[
    ///     0x1a00u16,
    ///     0x1a02,
    ///     0x1a04,
    ///     0x1a06,
    /// ]).await?;
    ///
    /// // ... this
    /// // subdevice.sdo_write(0x1c13, 0, 0u8).await?;
    /// // subdevice.sdo_write(0x1c13, 1, 0x1a00u16).await?;
    /// // subdevice.sdo_write(0x1c13, 2, 0x1a02u16).await?;
    /// // subdevice.sdo_write(0x1c13, 3, 0x1a04u16).await?;
    /// // subdevice.sdo_write(0x1c13, 4, 0x1a06u16).await?;
    /// // subdevice.sdo_write(0x1c13, 0, 4u8).await?;
    /// # Ok::<(), ethercrab::error::Error>(())
    /// # };
    /// ```
    pub async fn sdo_write_array<T>(&self, index: u16, values: impl AsRef<[T]>) -> Result<(), Error>
    where
        T: EtherCrabWireWrite,
    {
        Coe::new(self).sdo_write_array(index, values).await
    }

    /// Read all sub-indices of the given SDO.
    ///
    /// This method will return an error if the number of sub-indices in the SDO is greater than
    /// `MAX_ENTRIES.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use ethercrab::{
    /// #     error::Error, MainDevice, MainDeviceConfig, PduStorage, Timeouts, std::ethercat_now
    /// # };
    /// # static PDU_STORAGE: PduStorage<8, 32> = PduStorage::new();
    /// # let (_tx, _rx, pdu_loop) = PDU_STORAGE.try_split().expect("can only split once");
    /// # let maindevice = MainDevice::new(pdu_loop, Timeouts::default(), MainDeviceConfig::default());
    /// # async {
    /// # let mut group = maindevice
    /// #     .init_single_group::<8, 8>(ethercat_now)
    /// #     .await
    /// #     .expect("Init");
    /// let subdevice = group.subdevice(&maindevice, 0).expect("No subdevice!");
    ///
    /// // Reading the TxPDO assignment
    /// // This is equivalent to...
    /// let values = subdevice.sdo_read_array::<u16, 3>(0x1c13).await?;
    ///
    /// // ... this
    /// // let len: u8 = subdevice.sdo_read(0x1c13, 0).await?;
    /// // let value1: u16 = subdevice.sdo_read(0x1c13, 1).await?;
    /// // let value2: u16 = subdevice.sdo_read(0x1c13, 2).await?;
    /// // let value3: u16 = subdevice.sdo_read(0x1c13, 3).await?;
    ///
    /// # Ok::<(), ethercrab::error::Error>(())
    /// # };
    /// ```
    pub async fn sdo_read_array<T, const MAX_ENTRIES: usize>(
        &self,
        index: u16,
    ) -> Result<heapless::Vec<T, MAX_ENTRIES>, Error>
    where
        T: EtherCrabWireReadSized,
    {
        Coe::new(self).sdo_read_array(index).await
    }

    /// List out all of the CoE objects' addresses of kind `list_type`.
    ///
    /// For devices without CoE mailboxes, this will return `Ok(None)`.
    pub async fn sdo_info_object_description_list(
        &self,
        list_type: ObjectDescriptionListQuery,
    ) -> Result<Option<heapless::Vec<u16, /* # of u16s */ { u16::MAX as usize + 1 }>>, Error> {
        Coe::new(self)
            .sdo_info_object_description_list(list_type)
            .await
    }

    /// Count how many objects match each [`ObjectDescriptionListQuery`].
    pub async fn sdo_info_object_quantities(
        &self,
    ) -> Result<Option<ObjectDescriptionListQueryCounts>, Error> {
        Coe::new(self).sdo_info_object_quantities().await
    }
}

// General impl with no bounds
impl<'maindevice, S> SubDeviceRef<'maindevice, S> {
    pub(crate) fn new(
        maindevice: &'maindevice MainDevice<'maindevice>,
        configured_address: u16,
        state: S,
    ) -> Self {
        Self {
            maindevice,
            configured_address,
            state,
        }
    }

    /// Get the sub device status.
    pub(crate) async fn state(&self) -> Result<SubDeviceState, Error> {
        match self
            .read(RegisterAddress::AlStatus)
            .receive::<AlControl>(self.maindevice)
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
                        .receive::<AlStatusCode>(self.maindevice)
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
            .receive::<AlStatusCode>(self.maindevice);

        futures_lite::future::try_zip(self.state(), code).await
    }

    fn eeprom(&self) -> SubDeviceEeprom<DeviceEeprom> {
        SubDeviceEeprom::new(DeviceEeprom::new(self.maindevice, self.configured_address))
    }

    /// Read a register.
    ///
    /// Note that while this method is marked safe, raw alterations to SubDevice config or behaviour can
    /// break higher level interactions with EtherCrab.
    pub async fn register_read<T>(&self, register: impl Into<u16>) -> Result<T, Error>
    where
        T: EtherCrabWireReadSized,
    {
        self.read(register.into()).receive(self.maindevice).await
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
            .send_receive(self.maindevice, value)
            .await
    }

    pub(crate) async fn wait_for_state(&self, desired_state: SubDeviceState) -> Result<(), Error> {
        async {
            loop {
                let status = self
                    .read(RegisterAddress::AlStatus)
                    .ignore_wkc()
                    .receive::<AlControl>(self.maindevice)
                    .await?;

                if status.state == desired_state {
                    break Ok(());
                }

                self.maindevice.timeouts.loop_tick().await;
            }
        }
        .timeout(self.maindevice.timeouts.state_transition())
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
            .send_receive::<AlControl>(self.maindevice, AlControl::new(desired_state))
            .await?;

        if response.error {
            let error = self
                .read(RegisterAddress::AlStatusCode)
                .receive::<AlStatusCode>(self.maindevice)
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
            .send(self.maindevice, 2u16)
            .await?;

        self.write(RegisterAddress::SiiConfig)
            .send(self.maindevice, mode)
            .await?;

        Ok(())
    }
}
