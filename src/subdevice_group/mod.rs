//! A group of SubDevices.
//!
//! SubDevices can be divided into multiple groups to allow multiple tasks to run concurrently,
//! potentially at different tick rates.

mod group_id;
mod handle;
mod tx_rx_response;

use crate::{
    DcSync, MainDevice, RegisterAddress, SubDeviceState,
    al_control::AlControl,
    command::Command,
    ds402::SyncManagerAssignment,
    eeprom::types::{SyncManager, SyncManagerEnable},
    error::{DistributedClockError, Error, Item},
    fmt,
    pdi::PdiOffset,
    pdu_loop::{CreatedFrame, ReceivedPdu},
    subdevice::{
        IoRanges, SubDevice, SubDeviceRef, configuration::PdoDirection, pdi::SubDevicePdi,
    },
    sync_manager_channel::{Control, Direction, Enable, OperationMode},
    timer_factory::IntoTimeout,
};
use core::{
    cell::UnsafeCell,
    marker::PhantomData,
    ops::{Deref, Range},
    sync::atomic::AtomicUsize,
    time::Duration,
};
use ethercrab_wire::{EtherCrabWireRead, EtherCrabWireReadSized, EtherCrabWireSized, WireError};
use heapless::FnvIndexMap;

pub use self::group_id::GroupId;
pub use self::handle::SubDeviceGroupHandle;
pub use self::tx_rx_response::TxRxResponse;

static GROUP_ID: AtomicUsize = AtomicUsize::new(0);

/// The size of a DC sync PDU.
const DC_PDU_SIZE: usize = CreatedFrame::PDU_OVERHEAD_BYTES + u64::PACKED_LEN;

// MSRV: Remove when core SyncUnsafeCell is stabilised
#[derive(Debug)]
pub(crate) struct MySyncUnsafeCell<T: ?Sized>(pub UnsafeCell<T>);

impl<T> MySyncUnsafeCell<T> {
    pub fn new(inner: T) -> Self {
        Self(UnsafeCell::new(inner))
    }
}

unsafe impl<T: ?Sized + Sync> Sync for MySyncUnsafeCell<T> {}

impl<T: ?Sized> MySyncUnsafeCell<T> {
    /// Gets a mutable pointer to the wrapped value.
    ///
    /// This can be cast to a pointer of any kind.
    /// Ensure that the access is unique (no active references, mutable or not)
    /// when casting to `&mut T`, and ensure that there are no mutations
    /// or mutable aliases going on when casting to `&T`
    #[inline]
    pub const fn get(&self) -> *mut T {
        self.0.get()
    }

    /// Returns a mutable reference to the underlying data.
    ///
    /// This call borrows the `SyncUnsafeCell` mutably (at compile-time) which
    /// guarantees that we possess the only reference.
    #[inline]
    pub fn get_mut(&mut self) -> &mut T {
        self.0.get_mut()
    }
}

// TODO: Un-pub
/// TODO: Docs
pub struct PdiMappingBikeshedName<const I: usize, const O: usize, S = ()> {
    /// TODO: Docs.
    pub(crate) configured_address: u16,
    /// TODO: Docs.
    pub(crate) inputs: FnvIndexMap<u32, Range<usize>, I>,
    /// TODO: Docs.
    pub(crate) outputs: FnvIndexMap<u32, Range<usize>, O>,

    pub(crate) state: S,
}

impl<const I: usize, const O: usize, S> PdiMappingBikeshedName<I, O, S> {
    /// TODO: Docs
    /// TODO: Way better name
    pub fn with_subdevice<
        'maindevice,
        'group,
        const MAX_SUBDEVICES: usize,
        const MAX_PDI: usize,
        DC,
    >(
        self,
        maindevice: &'maindevice MainDevice<'maindevice>,
        group: &'group SubDeviceGroup<MAX_SUBDEVICES, MAX_PDI, impl HasPdi, DC>,
    ) -> Result<
        PdiMappingBikeshedName<I, O, SubDeviceRef<'maindevice, SubDevicePdi<'group, MAX_PDI>>>,
        Error,
    > {
        let subdevice =
            group.subdevice_by_configured_address(maindevice, self.configured_address)?;

        Ok(PdiMappingBikeshedName {
            configured_address: self.configured_address,
            inputs: self.inputs,
            outputs: self.outputs,
            state: subdevice,
        })
    }
}

impl<const I: usize, const O: usize, S> Deref for PdiMappingBikeshedName<I, O, S> {
    type Target = S;

    fn deref(&self) -> &Self::Target {
        &self.state
    }
}

impl<'maindevice, 'group, const I: usize, const O: usize, const MAX_PDI: usize>
    PdiMappingBikeshedName<I, O, SubDeviceRef<'maindevice, SubDevicePdi<'group, MAX_PDI>>>
{
    // TODO: Don't take a `u32` but an actual Object type
    /// TODO: Doc
    pub fn input<T>(&self, object: u32) -> Result<T, Error>
    where
        T: EtherCrabWireReadSized,
    {
        let size_bytes = (object & 0xff) as usize / 8;

        if size_bytes != T::PACKED_LEN {
            // TODO: This should probably be an Error::Mapping(IncorrectSize)
            return Err(Error::Internal);
        }

        let range = self.inputs.get(&object).ok_or_else(|| {
            // TODO: Return idk, Error::Mapping(NotFound)
            Error::Internal
        })?;

        let inputs = self.state.inputs_raw();

        let bytes = inputs.get(range.clone()).ok_or_else(|| {
            // TODO: Error::Mapping(OutOfRange) or something
            Error::Internal
        })?;

        Ok(T::unpack_from_slice(bytes)?)
    }
}

/// Sync Manager configuration.
#[derive(Debug, Copy, Clone)]
#[non_exhaustive]
pub struct SyncManagerConfig {
    /// Start address.
    ///
    /// This is mandatory in ESI files.
    pub start_addr: u16,

    /// Sync Manager default size. This MUST be set for mailbox sync managers.
    ///
    /// It will be computed and overwritten by EtherCrab for process data inputs/outputs based on
    /// the mapping lengths.
    pub size: u16,

    /// Configuration for this Sync Manager.
    ///
    /// This will be parsed from the `ControlByte` property in the ESI file.
    pub control: Control,

    /// Whether to enable this sync manager or not.
    pub enabled: bool,
}

impl SyncManagerConfig {
    /// Create a mailbox sync manager config with given size, start address and direction.
    pub fn mailbox(start_addr: u16, size: u16, direction: Direction) -> Self {
        SyncManagerConfig {
            start_addr,
            size,
            enabled: true,
            control: Control {
                operation_mode: OperationMode::Mailbox,
                direction,
                ..Control::default()
            },
        }
    }

    /// Create a mailbox sync manager config with given size, start address and direction.
    pub fn process_data(start_addr: u16, direction: Direction) -> Self {
        SyncManagerConfig {
            start_addr,
            // Computed later when we do all the PDO mappings.
            size: 0,
            enabled: true,
            control: Control {
                operation_mode: OperationMode::ProcessData,
                direction,
                ..Control::default()
            },
        }
    }

    // TODO: Bikeshed: The `SyncManager` should probably not be an EEPROM-related type and just made generic.
    pub(crate) fn bikeshed_into_eeprom_type(&self) -> crate::eeprom::types::SyncManager {
        SyncManager {
            start_addr: self.start_addr,
            length_bytes: self.size,
            control: self.control,
            enable: if self.enabled {
                SyncManagerEnable::ENABLE
            } else {
                SyncManagerEnable::empty()
            },
        }
    }
}

/// FMMU configuration.
#[derive(Debug, Copy, Clone)]
#[non_exhaustive]
pub struct FmmuConfig {
    /// FMMU kind.
    pub kind: FmmuKind,

    /// Sync manager index to assign to this FMMU. Leave as `None` if unsure.
    pub sync_manager: Option<u8>,
}

/// FMMU kind.
#[derive(Debug, Copy, Clone)]
pub enum FmmuKind {
    /// Process data outputs from MainDevice.
    Outputs,
    /// Process data inputs into MainDevice.
    Inputs,
    /// Mailbox state.
    MBoxState,
    /// Dynamic inputs.
    DynamicInputs,
    /// Dynamic outputs.
    DynamicOutputs,
}

/// TODO: Doc
#[derive(Debug, Copy, Clone)]
#[non_exhaustive]
pub struct MappingConfig<'a> {
    /// Input mappings (SubDevice -> MainDevice).
    pub inputs: &'a [SyncManagerAssignment<'a>],

    /// Output mappings (MainDevice -> SubDevice).
    pub outputs: &'a [SyncManagerAssignment<'a>],

    /// FMMU configuration.
    ///
    /// When writing configuration manually, this field can be left empty (`&[]`) to let EtherCrab
    /// compute FMMU assignments automatically.
    pub fmmus: &'a [FmmuConfig],

    /// Sync manager config.
    ///
    /// When writing configuration manually, this field can be left empty (`&[]`) to let EtherCrab
    /// compute Sync Manager assignments automatically.
    pub sync_managers: &'a [SyncManagerConfig],
}

impl<'a> MappingConfig<'a> {
    /// Create a new PDO mapping config with both inputs and outputs.
    pub const fn new(
        inputs: &'a [SyncManagerAssignment<'a>],
        outputs: &'a [SyncManagerAssignment<'a>],
    ) -> Self {
        Self {
            inputs,
            outputs,
            fmmus: &[],
            sync_managers: &[],
        }
    }

    /// Create a new PDO mapping config with only inputs (SubDevice into MainDevice).
    pub const fn inputs(inputs: &'a [SyncManagerAssignment<'a>]) -> Self {
        Self {
            inputs,
            outputs: &[],
            fmmus: &[],
            sync_managers: &[],
        }
    }

    /// Create a new PDO mapping config with only outputs (MainDevice out to SubDevice).
    pub const fn outputs(outputs: &'a [SyncManagerAssignment<'a>]) -> Self {
        Self {
            inputs: &[],
            outputs,
            fmmus: &[],
            sync_managers: &[],
        }
    }

    /// Write configuration to SubDevice CoE SDOs.
    pub async fn configure_sdos(
        &self,
        subdevice: &SubDeviceRef<'_, &mut SubDevice>,
    ) -> Result<(), Error> {
        fmt::debug!("Write PDO mapping config");

        for assignment in self.inputs.iter() {
            for mapping in assignment.mappings.iter() {
                fmt::debug!(
                    "--> Inputs {:#06x} {:#010x?}",
                    mapping.index,
                    mapping.objects
                );

                subdevice
                    .sdo_write_array(mapping.index, mapping.objects)
                    .await?;
            }
        }

        for assignment in self.outputs.iter() {
            for mapping in assignment.mappings.iter() {
                fmt::debug!(
                    "--> Outputs {:#06x} {:#010x?}",
                    mapping.index,
                    mapping.objects
                );

                subdevice
                    .sdo_write_array(mapping.index, mapping.objects)
                    .await?;
            }
        }

        for (idx, assignment) in self.inputs.iter().chain(self.outputs.iter()).enumerate() {
            // First two SMs will be mailbox in/out if we have CoE support, so we start from 0x1c12
            // instead of 0x1c10.
            let fallback = 2 + idx as u16;

            let sm_index = 0x1c10 + assignment.sync_manager.map(u16::from).unwrap_or(fallback);

            fmt::debug!("--> SM assignment {:#06x}", sm_index);

            subdevice.sdo_write(sm_index, 0, 0u8).await?;

            let mut count = 0u8;

            for (sub_index, mapping) in assignment.mappings.iter().enumerate() {
                // Sub indices start at 8
                let sub_index = sub_index as u8 + 1;

                fmt::debug!(
                    "----> Object {:#06x} at sub-index {}",
                    mapping.index,
                    sub_index
                );

                subdevice
                    .sdo_write(sm_index, sub_index, mapping.index)
                    .await?;

                count += 1;
            }

            subdevice.sdo_write(sm_index, 0, count).await?;
        }

        Ok(())
    }

    /// TODO: Doc
    pub fn pdi_mapping<const I: usize, const O: usize>(
        &self,
        subdevice: &impl Deref<Target = SubDevice>,
    ) -> PdiMappingBikeshedName<I, O> {
        let mut inputs = FnvIndexMap::<_, _, I>::new();
        let mut outputs = FnvIndexMap::<_, _, O>::new();

        fn push_mapping<const N: usize>(
            inputs: &mut FnvIndexMap<u32, Range<usize>, N>,
            position_accumulator: usize,
            object: &u32,
        ) -> usize {
            let object = *object;

            let size_bytes = (object & 0xffff) as usize / 8;

            let range = position_accumulator..(position_accumulator + size_bytes);

            assert_eq!(
                inputs.insert(object, range),
                Ok(None),
                "multiple mappings of object {:#06x}",
                object
            );

            position_accumulator + size_bytes
        }

        self.inputs
            .iter()
            .flat_map(|sm| sm.mappings)
            .flat_map(|mapping| mapping.objects)
            .fold(0usize, |position_accumulator, object| {
                push_mapping(&mut inputs, position_accumulator, object)
            });

        self.outputs
            .iter()
            .flat_map(|sm| sm.mappings)
            .flat_map(|mapping| mapping.objects)
            .fold(0usize, |position_accumulator, object| {
                push_mapping(&mut outputs, position_accumulator, object)
            });

        PdiMappingBikeshedName {
            configured_address: subdevice.configured_address(),
            inputs,
            outputs,
            state: (),
        }
    }
}

/// A typestate for [`SubDeviceGroup`] representing a group that is shut down.
///
/// This corresponds to the EtherCAT states INIT.
#[derive(Copy, Clone, Debug)]
pub struct Init;

/// A typestate for [`SubDeviceGroup`] representing a group that is undergoing initialisation.
///
/// This corresponds to the EtherCAT states INIT and PRE-OP.
#[derive(Copy, Clone, Debug)]
pub struct PreOp;

/// The same as [`PreOp`] but with access to PDI methods. All SubDevice configuration should be complete
/// at this point.
#[derive(Copy, Clone, Debug)]
pub struct PreOpPdi;

/// A typestate for [`SubDeviceGroup`] representing a group that is in SAFE-OP.
#[derive(Copy, Clone, Debug)]
pub struct SafeOp;

/// A typestate for [`SubDeviceGroup`] representing a group that is in OP.
#[derive(Copy, Clone, Debug)]
pub struct Op;

/// A typestate for [`SubDeviceGroup`]s that do not have a Distributed Clock configuration
#[derive(Copy, Clone, Debug)]
pub struct NoDc;

/// A typestate for [`SubDeviceGroup`]s that have a configured Distributed Clock.
///
/// This typestate can be entered by calling [`SubDeviceGroup::configure_dc_sync`].
#[derive(Copy, Clone, Debug)]
pub struct HasDc {
    sync0_period: u64,
    sync0_shift: u64,
    /// Configured address of the DC reference SubDevice.
    reference: u16,
}

/// Marker trait for `SubDeviceGroup` typestates where all SubDevices have a PDI.
#[doc(hidden)]
pub trait HasPdi {}

impl HasPdi for PreOpPdi {}
impl HasPdi for SafeOp {}
impl HasPdi for Op {}

#[doc(hidden)]
pub trait IsPreOp {}

impl IsPreOp for PreOp {}
impl IsPreOp for PreOpPdi {}

#[derive(Default)]
struct GroupInner<const MAX_SUBDEVICES: usize> {
    subdevices: heapless::Vec<SubDevice, MAX_SUBDEVICES>,
    pdi_start: PdiOffset,
}

const CYCLIC_OP_ENABLE: u8 = 0b0000_0001;
const SYNC0_ACTIVATE: u8 = 0b0000_0010;
const SYNC1_ACTIVATE: u8 = 0b0000_0100;

/// Group distributed clock configuration.
#[derive(Default, Debug, Copy, Clone)]
pub struct DcConfiguration {
    /// How long the SubDevices in the group should wait before starting SYNC0 pulse generation.
    pub start_delay: Duration,

    /// SYNC0 cycle time.
    ///
    /// SubDevices with an `AssignActivate` value of `0x0300` in their ESI definition should set
    /// this value.
    pub sync0_period: Duration,

    /// Shift time relative to SYNC0 pulse.
    pub sync0_shift: Duration,
}

/// Information useful to a process data cycle.
#[derive(Debug, Copy, Clone)]
pub struct CycleInfo {
    /// Distributed Clock System time in nanoseconds.
    pub dc_system_time: u64,

    /// The time to wait before starting the next process data cycle.
    ///
    /// This duration is calculated based on the [`sync0_period`](DcConfiguration::sync0_period) and
    /// [`sync0_shift`](DcConfiguration::sync0_shift) passed into [`SubDeviceGroup::configure_dc_sync`]
    /// and is meant to be used to accurately synchronise the MainDevice process data cycle with the
    /// DC system time.
    pub next_cycle_wait: Duration,

    /// The difference between the SYNC0 pulse and when the current cycle's data was received by the
    /// DC reference SubDevice.
    pub cycle_start_offset: Duration,
}

/// A group of one or more EtherCAT SubDevices.
///
/// Groups are created during EtherCrab initialisation, and are the only way to access individual
/// SubDevice PDI sections.
#[doc(alias = "SlaveGroup")]
pub struct SubDeviceGroup<const MAX_SUBDEVICES: usize, const MAX_PDI: usize, S = PreOp, DC = NoDc> {
    id: GroupId,
    pdi: spin::rwlock::RwLock<MySyncUnsafeCell<[u8; MAX_PDI]>, crate::SpinStrategy>,
    /// The number of bytes at the beginning of the PDI reserved for SubDevice inputs.
    read_pdi_len: usize,
    /// The total length (I and O) of the PDI for this group.
    pdi_len: usize,
    inner: MySyncUnsafeCell<GroupInner<MAX_SUBDEVICES>>,
    dc_conf: DC,
    _state: PhantomData<S>,
}

impl<const MAX_SUBDEVICES: usize, const MAX_PDI: usize, DC>
    SubDeviceGroup<MAX_SUBDEVICES, MAX_PDI, PreOp, DC>
{
    async fn configure_fmmus2<'fun>(
        &mut self,
        maindevice: &MainDevice<'_>,
        mut configure: impl AsyncFnMut(
            SubDeviceRef<'_, &mut SubDevice>,
            usize,
        ) -> Result<Option<MappingConfig<'fun>>, Error>,
    ) -> Result<(), Error> {
        let inner = self.inner.get_mut();

        let mut pdi_position = inner.pdi_start;

        let mut configs = heapless::Vec::<_, MAX_SUBDEVICES>::new();

        for (i, subdevice) in inner.subdevices.iter_mut().enumerate() {
            // SAFETY: `configs` uses `MAX_SUBDEVICES` so should be the same length as or shorter
            // than `inner.subdevices`.
            fmt::unwrap!(
                configs.push(
                    configure(
                        SubDeviceRef::new(maindevice, subdevice.configured_address(), subdevice),
                        i,
                    )
                    .await?,
                )
            );
        }

        fmt::debug!(
            "Going to configure group with {} SubDevice(s), starting PDI offset {:#010x}",
            inner.subdevices.len(),
            inner.pdi_start.start_address
        );

        // Configure master read PDI mappings in the first section of the PDI
        for (subdevice, config) in inner.subdevices.iter_mut().zip(configs.iter()) {
            // We're in PRE-OP at this point
            pdi_position = SubDeviceRef::new(maindevice, subdevice.configured_address(), subdevice)
                .configure_fmmus(
                    pdi_position,
                    inner.pdi_start.start_address,
                    PdoDirection::MainDeviceRead,
                    config,
                )
                .await?;
        }

        self.read_pdi_len = (pdi_position.start_address - inner.pdi_start.start_address) as usize;

        fmt::debug!("SubDevice mailboxes configured and init hooks called");

        // We configured all read PDI mappings as a contiguous block in the previous loop. Now we'll
        // configure the write mappings in a separate loop. This means we have IIIIOOOO instead of
        // IOIOIO.
        for (subdevice, config) in inner.subdevices.iter_mut().zip(configs.iter()) {
            let addr = subdevice.configured_address();

            let mut subdevice_config = SubDeviceRef::new(maindevice, addr, subdevice);

            // Still in PRE-OP
            pdi_position = subdevice_config
                .configure_fmmus(
                    pdi_position,
                    inner.pdi_start.start_address,
                    PdoDirection::MainDeviceWrite,
                    config,
                )
                .await?;
        }

        fmt::debug!("SubDevice FMMUs configured for group. Able to move to SAFE-OP");

        self.pdi_len = (pdi_position.start_address - inner.pdi_start.start_address) as usize;

        fmt::debug!(
            "Group PDI length: start {:#010x}, {} total bytes ({} input bytes)",
            inner.pdi_start.start_address,
            self.pdi_len,
            self.read_pdi_len
        );

        if self.pdi_len > MAX_PDI {
            return Err(Error::PdiTooLong {
                max_length: MAX_PDI,
                desired_length: self.pdi_len,
            });
        }

        Ok(())
    }

    /// Configure read/write FMMUs and PDI for this group.
    async fn configure_fmmus(&mut self, maindevice: &MainDevice<'_>) -> Result<(), Error> {
        let inner = self.inner.get_mut();

        let mut pdi_position = inner.pdi_start;

        fmt::debug!(
            "Going to configure group with {} SubDevice(s), starting PDI offset {:#010x}",
            inner.subdevices.len(),
            inner.pdi_start.start_address
        );

        // Configure master read PDI mappings in the first section of the PDI
        for subdevice in inner.subdevices.iter_mut() {
            // We're in PRE-OP at this point
            pdi_position = SubDeviceRef::new(maindevice, subdevice.configured_address(), subdevice)
                .configure_fmmus(
                    pdi_position,
                    inner.pdi_start.start_address,
                    PdoDirection::MainDeviceRead,
                    &None,
                )
                .await?;
        }

        self.read_pdi_len = (pdi_position.start_address - inner.pdi_start.start_address) as usize;

        fmt::debug!("SubDevice mailboxes configured and init hooks called");

        // We configured all read PDI mappings as a contiguous block in the previous loop. Now we'll
        // configure the write mappings in a separate loop. This means we have IIIIOOOO instead of
        // IOIOIO.
        for subdevice in inner.subdevices.iter_mut() {
            let addr = subdevice.configured_address();

            let mut subdevice_config = SubDeviceRef::new(maindevice, addr, subdevice);

            // Still in PRE-OP
            pdi_position = subdevice_config
                .configure_fmmus(
                    pdi_position,
                    inner.pdi_start.start_address,
                    PdoDirection::MainDeviceWrite,
                    &None,
                )
                .await?;
        }

        fmt::debug!("SubDevice FMMUs configured for group. Able to move to SAFE-OP");

        self.pdi_len = (pdi_position.start_address - inner.pdi_start.start_address) as usize;

        fmt::debug!(
            "Group PDI length: start {:#010x}, {} total bytes ({} input bytes)",
            inner.pdi_start.start_address,
            self.pdi_len,
            self.read_pdi_len
        );

        if self.pdi_len > MAX_PDI {
            return Err(Error::PdiTooLong {
                max_length: MAX_PDI,
                desired_length: self.pdi_len,
            });
        }

        Ok(())
    }

    /// Borrow an individual SubDevice.
    #[deny(clippy::panic)]
    #[doc(alias = "slave")]
    pub fn subdevice<'maindevice, 'group>(
        &'group self,
        maindevice: &'maindevice MainDevice<'maindevice>,
        index: usize,
    ) -> Result<SubDeviceRef<'maindevice, &'group SubDevice>, Error> {
        let subdevice = self.inner().subdevices.get(index).ok_or(Error::NotFound {
            item: Item::SubDevice,
            index: Some(index),
        })?;

        Ok(SubDeviceRef::new(
            maindevice,
            subdevice.configured_address(),
            subdevice,
        ))
    }

    // /// Borrow an individual SubDevice, searched for by configured station address.
    // #[deny(clippy::panic)]
    // pub(crate) fn subdevice_by_configured_address<'maindevice, 'group>(
    //     &'group self,
    //     maindevice: &'maindevice MainDevice<'maindevice>,
    //     configured_address: u16,
    // ) -> Result<SubDeviceRef<'maindevice, &'group SubDevice>, Error> {
    //     let subdevice = self
    //         .inner()
    //         .subdevices
    //         .iter()
    //         .find(|sd| sd.configured_address() == configured_address)
    //         .ok_or(Error::NotFound {
    //             item: Item::SubDevice,
    //             index: Some(configured_address.into()),
    //         })?;

    //     Ok(SubDeviceRef::new(
    //         maindevice,
    //         subdevice.configured_address(),
    //         subdevice,
    //     ))
    // }

    /// Transition the group from PRE-OP -> SAFE-OP -> OP.
    ///
    /// To transition individually from PRE-OP to SAFE-OP, then SAFE-OP to OP, see
    /// [`SubDeviceGroup::into_safe_op`].
    pub async fn into_op(
        self,
        maindevice: &MainDevice<'_>,
    ) -> Result<SubDeviceGroup<MAX_SUBDEVICES, MAX_PDI, Op, DC>, Error> {
        let self_ = self.into_safe_op(maindevice).await?;

        self_.into_op(maindevice).await
    }

    // NOTE: This just goes PRE-OP -> PRE-OP + PDI. What about into op and into safe op?
    /// TODO: Docs
    pub async fn into_pre_op_pdi_with_config<'fun>(
        mut self,
        maindevice: &MainDevice<'_>,
        configure: impl AsyncFnMut(
            SubDeviceRef<'_, &mut SubDevice>,
            usize,
        ) -> Result<Option<MappingConfig<'fun>>, Error>,
    ) -> Result<SubDeviceGroup<MAX_SUBDEVICES, MAX_PDI, PreOpPdi, DC>, Error>
    where
        DC: 'fun,
    {
        self.configure_fmmus2(maindevice, configure).await?;

        Ok(SubDeviceGroup {
            id: self.id,
            pdi: self.pdi,
            read_pdi_len: self.read_pdi_len,
            pdi_len: self.pdi_len,
            inner: self.inner,
            dc_conf: self.dc_conf,
            _state: PhantomData,
        })
    }

    /// Configure FMMUs, but leave the group in [`PreOp`] state.
    ///
    /// This method is used to obtain access to the group's PDI and related functionality. All SDO
    /// and other configuration should be complete at this point otherwise issues with cyclic data
    /// may occur (e.g. incorrect lengths, misplaced fields, etc).
    pub async fn into_pre_op_pdi(
        mut self,
        maindevice: &MainDevice<'_>,
    ) -> Result<SubDeviceGroup<MAX_SUBDEVICES, MAX_PDI, PreOpPdi, DC>, Error> {
        self.configure_fmmus(maindevice).await?;

        Ok(SubDeviceGroup {
            id: self.id,
            pdi: self.pdi,
            read_pdi_len: self.read_pdi_len,
            pdi_len: self.pdi_len,
            inner: self.inner,
            dc_conf: self.dc_conf,
            _state: PhantomData,
        })
    }

    /// Transition the SubDevice group from PRE-OP to SAFE-OP.
    pub async fn into_safe_op(
        self,
        maindevice: &MainDevice<'_>,
    ) -> Result<SubDeviceGroup<MAX_SUBDEVICES, MAX_PDI, SafeOp, DC>, Error> {
        let self_ = self.into_pre_op_pdi(maindevice).await?;

        // We're done configuring FMMUs, etc, now we can request all SubDevices in this group go into
        // SAFE-OP
        self_
            .transition_to(maindevice, SubDeviceState::SafeOp)
            .await
    }

    /// Transition all SubDevices in the group from PRE-OP to INIT.
    pub async fn into_init(
        self,
        maindevice: &MainDevice<'_>,
    ) -> Result<SubDeviceGroup<MAX_SUBDEVICES, MAX_PDI, Init, DC>, Error> {
        self.transition_to(maindevice, SubDeviceState::Init).await
    }

    /// Get an iterator over all SubDevices in this group.
    pub fn iter<'group, 'maindevice>(
        &'group self,
        maindevice: &'maindevice MainDevice<'maindevice>,
    ) -> impl Iterator<Item = SubDeviceRef<'maindevice, &'group SubDevice>> {
        self.inner()
            .subdevices
            .iter()
            .map(|sd| SubDeviceRef::new(maindevice, sd.configured_address, sd))
    }

    /// Get a mutable iterator over all SubDevices in this group
    pub fn iter_mut<'group, 'maindevice>(
        &'group mut self,
        maindevice: &'maindevice MainDevice<'maindevice>,
    ) -> impl Iterator<Item = SubDeviceRef<'maindevice, &'group mut SubDevice>> {
        self.inner
            .get_mut()
            .subdevices
            .iter_mut()
            .map(|sd| SubDeviceRef::new(maindevice, sd.configured_address, sd))
    }
}

impl<const MAX_SUBDEVICES: usize, const MAX_PDI: usize, S, DC>
    SubDeviceGroup<MAX_SUBDEVICES, MAX_PDI, S, DC>
where
    S: IsPreOp,
{
    /// Configure Distributed Clock SYNC0 for all SubDevices in this group.
    ///
    /// All configured times in the [`DcConfiguration`] struct must be under `u32::MAX` nanoseconds.
    /// This means that e.g. the sync start delay must not be greater than rougly 4.2 seconds.
    ///
    /// # Errors
    ///
    /// This method will return with a
    /// [`Error::DistributedClock(DistributedClockError::NoReference)`](Error::DistributedClock)
    /// error if no DC reference SubDevice is present on the network.
    ///
    /// This method will also return an error if any of the [`DcConfiguration`] struct's fields hold
    /// a value greater than `u32::MAX` nanoseconds.
    pub async fn configure_dc_sync(
        self,
        maindevice: &MainDevice<'_>,
        dc_conf: DcConfiguration,
    ) -> Result<SubDeviceGroup<MAX_SUBDEVICES, MAX_PDI, PreOpPdi, HasDc>, Error> {
        fmt::debug!("Configuring distributed clocks for group");

        let Some(reference) = maindevice.dc_ref_address() else {
            fmt::error!("No DC reference clock SubDevice present, unable to configure DC");

            return Err(DistributedClockError::NoReference.into());
        };

        let DcConfiguration {
            start_delay,
            sync0_period,
            sync0_shift,
        } = dc_conf;

        // Coerce generics into concrete `PreOp` type as we don't need the PDI to configure the DC.
        let self_ = SubDeviceGroup {
            id: self.id,
            pdi: self.pdi,
            read_pdi_len: self.read_pdi_len,
            pdi_len: self.pdi_len,
            inner: self.inner,
            dc_conf: NoDc,
            _state: PhantomData::<PreOp>,
        };

        // Only configure DC for those devices that want and support it
        let dc_devices = self_.iter(maindevice).filter(|subdevice| {
            subdevice.dc_support().any() && !matches!(subdevice.dc_sync(), DcSync::Disabled)
        });

        let system_time = SubDeviceRef::new(maindevice, reference, ())
            .register_read::<u64>(RegisterAddress::DcSystemTime)
            .await?;

        // Kinda weird converting to/from u32 but these values must not exceed u32::MAX
        let sync0_period = u64::from(u32::try_from(sync0_period.as_nanos())?);

        let first_pulse_delay = u64::from(u32::try_from(start_delay.as_nanos())?);

        for subdevice in dc_devices {
            fmt::debug!(
                "--> Configuring SubDevice {:#06x} {} DC mode {}",
                subdevice.configured_address(),
                subdevice.name(),
                subdevice.dc_sync()
            );

            // Disable cyclic op, ignore WKC
            subdevice
                .write(RegisterAddress::DcSyncActive)
                .ignore_wkc()
                .send(maindevice, 0u8)
                .await?;

            // Write access to EtherCAT
            subdevice
                .write(RegisterAddress::DcCyclicUnitControl)
                .send(maindevice, 0u8)
                .await?;

            // Round first pulse time to a whole number of cycles
            let start_time = (system_time + first_pulse_delay) / sync0_period * sync0_period;

            fmt::debug!("--> Computed DC sync start time: {}", start_time);

            subdevice
                .write(RegisterAddress::DcSyncStartTime)
                .send(maindevice, start_time)
                .await?;

            // Cycle time in nanoseconds
            subdevice
                .write(RegisterAddress::DcSync0CycleTime)
                .send(maindevice, sync0_period)
                .await?;

            let flags = if let DcSync::Sync01 { sync1_period } = subdevice.dc_sync() {
                let sync1_period = u64::from(u32::try_from(sync1_period.as_nanos())?);

                subdevice
                    .write(RegisterAddress::DcSync1CycleTime)
                    .send(maindevice, sync1_period)
                    .await?;

                SYNC1_ACTIVATE | SYNC0_ACTIVATE | CYCLIC_OP_ENABLE
            } else {
                SYNC0_ACTIVATE | CYCLIC_OP_ENABLE
            };

            subdevice
                .write(RegisterAddress::DcSyncActive)
                .send(maindevice, flags)
                .await?;
        }

        Ok(SubDeviceGroup {
            id: self_.id,
            pdi: self_.pdi,
            read_pdi_len: self_.read_pdi_len,
            pdi_len: self_.pdi_len,
            inner: self_.inner,
            dc_conf: HasDc {
                sync0_period: sync0_period,
                sync0_shift: sync0_shift.as_nanos() as u64,
                reference,
            },
            _state: PhantomData,
        })
    }
}

impl<const MAX_SUBDEVICES: usize, const MAX_PDI: usize, DC>
    SubDeviceGroup<MAX_SUBDEVICES, MAX_PDI, PreOpPdi, DC>
{
    /// Transition the SubDevice group from PRE-OP to SAFE-OP.
    pub async fn into_safe_op(
        self,
        maindevice: &MainDevice<'_>,
    ) -> Result<SubDeviceGroup<MAX_SUBDEVICES, MAX_PDI, SafeOp, DC>, Error> {
        self.transition_to(maindevice, SubDeviceState::SafeOp).await
    }

    /// Transition all SubDevices in the group from PRE-OP to SAFE-OP, then to OP.
    ///
    /// This is a convenience method that calls [`into_safe_op`](SubDeviceGroup::into_safe_op) then
    /// [`into_op`](SubDeviceGroup::into_op).
    pub async fn into_op(
        self,
        maindevice: &MainDevice<'_>,
    ) -> Result<SubDeviceGroup<MAX_SUBDEVICES, MAX_PDI, Op, DC>, Error> {
        let self_ = self.into_safe_op(maindevice).await?;

        self_.transition_to(maindevice, SubDeviceState::Op).await
    }

    /// Like [`into_op`](SubDeviceGroup::into_op), however does not wait for all SubDevices to enter
    /// OP state.
    ///
    /// This allows the application process data loop to be started, so as to e.g. not time out
    /// watchdogs, or provide valid data to prevent DC sync errors.
    ///
    /// The group's state can be checked by testing the result of a `tx_rx_*` call using methods on
    /// the [`TxRxResponse`] struct.
    pub async fn request_into_op(
        self,
        maindevice: &MainDevice<'_>,
    ) -> Result<SubDeviceGroup<MAX_SUBDEVICES, MAX_PDI, Op, DC>, Error> {
        let self_ = self.into_safe_op(maindevice).await?;

        self_.request_into_op(maindevice).await
    }

    /// Transition all SubDevices in the group from PRE-OP to INIT.
    pub async fn into_init(
        self,
        maindevice: &MainDevice<'_>,
    ) -> Result<SubDeviceGroup<MAX_SUBDEVICES, MAX_PDI, Init, DC>, Error> {
        self.transition_to(maindevice, SubDeviceState::Init).await
    }
}

impl<const MAX_SUBDEVICES: usize, const MAX_PDI: usize, DC>
    SubDeviceGroup<MAX_SUBDEVICES, MAX_PDI, SafeOp, DC>
{
    /// Transition all SubDevices in the group from SAFE-OP to OP.
    pub async fn into_op(
        self,
        maindevice: &MainDevice<'_>,
    ) -> Result<SubDeviceGroup<MAX_SUBDEVICES, MAX_PDI, Op, DC>, Error> {
        self.transition_to(maindevice, SubDeviceState::Op).await
    }

    /// Transition all SubDevices in the group from SAFE-OP to PRE-OP.
    pub async fn into_pre_op(
        self,
        maindevice: &MainDevice<'_>,
    ) -> Result<SubDeviceGroup<MAX_SUBDEVICES, MAX_PDI, PreOp, DC>, Error> {
        self.transition_to(maindevice, SubDeviceState::PreOp).await
    }

    /// Like [`into_op`](SubDeviceGroup::into_op), however does not wait for all SubDevices to enter OP
    /// state.
    ///
    /// This allows the application process data loop to be started, so as to e.g. not time out
    /// watchdogs, or provide valid data to prevent DC sync errors.
    ///
    /// The group's state can be checked by testing the result of a `tx_rx_*` call using methods on
    /// the [`TxRxResponse`] struct.
    pub async fn request_into_op(
        mut self,
        maindevice: &MainDevice<'_>,
    ) -> Result<SubDeviceGroup<MAX_SUBDEVICES, MAX_PDI, Op, DC>, Error> {
        for subdevice in self.inner.get_mut().subdevices.iter_mut() {
            SubDeviceRef::new(maindevice, subdevice.configured_address(), subdevice)
                .request_subdevice_state_nowait(SubDeviceState::Op)
                .await?;
        }

        Ok(SubDeviceGroup {
            id: self.id,
            pdi: self.pdi,
            read_pdi_len: self.read_pdi_len,
            pdi_len: self.pdi_len,
            inner: self.inner,
            dc_conf: self.dc_conf,
            _state: PhantomData,
        })
    }
}

impl<const MAX_SUBDEVICES: usize, const MAX_PDI: usize, DC>
    SubDeviceGroup<MAX_SUBDEVICES, MAX_PDI, Op, DC>
{
    /// Transition all SubDevices in the group from OP to SAFE-OP.
    pub async fn into_safe_op(
        self,
        maindevice: &MainDevice<'_>,
    ) -> Result<SubDeviceGroup<MAX_SUBDEVICES, MAX_PDI, SafeOp, DC>, Error> {
        self.transition_to(maindevice, SubDeviceState::SafeOp).await
    }
}

impl<const MAX_SUBDEVICES: usize, const MAX_PDI: usize, S> Default
    for SubDeviceGroup<MAX_SUBDEVICES, MAX_PDI, S>
{
    fn default() -> Self {
        Self {
            id: GroupId(GROUP_ID.fetch_add(1, core::sync::atomic::Ordering::Relaxed)),
            pdi: spin::rwlock::RwLock::new(MySyncUnsafeCell::new([0u8; MAX_PDI])),
            read_pdi_len: Default::default(),
            pdi_len: Default::default(),
            inner: MySyncUnsafeCell::new(GroupInner::default()),
            dc_conf: NoDc,
            _state: PhantomData,
        }
    }
}

impl<const MAX_SUBDEVICES: usize, const MAX_PDI: usize, S, DC>
    SubDeviceGroup<MAX_SUBDEVICES, MAX_PDI, S, DC>
{
    fn inner(&self) -> &GroupInner<MAX_SUBDEVICES> {
        unsafe { &*self.inner.get() }
    }

    /// Get the number of SubDevices in this group.
    pub fn len(&self) -> usize {
        self.inner().subdevices.len()
    }

    /// Check whether this SubDevice group is empty or not.
    pub fn is_empty(&self) -> bool {
        self.inner().subdevices.is_empty()
    }

    /// Check if all SubDevices in the group are the given desired state.
    async fn is_state(
        &self,
        maindevice: &MainDevice<'_>,
        desired_state: SubDeviceState,
    ) -> Result<bool, Error> {
        fmt::trace!("Check group state");

        let mut subdevices = self.inner().subdevices.iter();

        let mut total_checks = 0;

        // Send as many frames as required to check statuses of all subdevices
        loop {
            let mut frame = maindevice.pdu_loop.alloc_frame()?;

            let (rest, num_in_this_frame) = push_state_checks(subdevices, &mut frame)?;

            subdevices = rest;

            // Nothing to send, we've checked all SDs
            if num_in_this_frame == 0 {
                fmt::trace!("--> No more state checks, pushed {}", total_checks);

                break;
            }

            total_checks += num_in_this_frame;

            let frame = frame.mark_sendable(
                &maindevice.pdu_loop,
                maindevice.timeouts.pdu,
                maindevice.config.retry_behaviour.retry_count(),
            );

            maindevice.pdu_loop.wake_sender();

            let received = frame.await?;

            for pdu in received.into_pdu_iter() {
                let pdu = pdu?;

                let result = AlControl::unpack_from_slice(&pdu)?;

                // Return from this fn as soon as the first undesired state is found
                if result.state != desired_state {
                    return Ok(false);
                }
            }
        }

        // Just sanity checking myself
        debug_assert_eq!(total_checks, self.len());

        Ok(true)
    }

    /// Wait for all SubDevices in this group to transition to the given state.
    async fn wait_for_state(
        &self,
        maindevice: &MainDevice<'_>,
        desired_state: SubDeviceState,
    ) -> Result<(), Error> {
        async {
            loop {
                if self.is_state(maindevice, desired_state).await? {
                    break Ok(());
                }

                maindevice.timeouts.loop_tick().await;
            }
        }
        .timeout(maindevice.timeouts.state_transition)
        .await
    }

    /// Transition to a new state.
    async fn transition_to<TO>(
        mut self,
        maindevice: &MainDevice<'_>,
        desired_state: SubDeviceState,
    ) -> Result<SubDeviceGroup<MAX_SUBDEVICES, MAX_PDI, TO, DC>, Error> {
        // We're done configuring FMMUs, etc, now we can request all SubDevices in this group go into
        // SAFE-OP
        for subdevice in self.inner.get_mut().subdevices.iter_mut() {
            SubDeviceRef::new(maindevice, subdevice.configured_address(), subdevice)
                .request_subdevice_state_nowait(desired_state)
                .await?;
        }

        fmt::debug!("Waiting for group state {}", desired_state);

        self.wait_for_state(maindevice, desired_state).await?;

        fmt::debug!("--> Group reached state {}", desired_state);

        Ok(SubDeviceGroup {
            id: self.id,
            pdi: self.pdi,
            read_pdi_len: self.read_pdi_len,
            pdi_len: self.pdi_len,
            inner: self.inner,
            dc_conf: self.dc_conf,
            _state: PhantomData,
        })
    }
}

fn push_state_checks<'group, 'sto, I>(
    mut subdevices: I,
    frame: &mut CreatedFrame<'sto>,
) -> Result<(I, usize), Error>
where
    I: Iterator<Item = &'group SubDevice>,
{
    let mut num_in_this_frame = 0;

    while frame.can_push_pdu_payload(AlControl::PACKED_LEN) {
        let Some(sd) = subdevices.next() else {
            break;
        };

        // A too-long error here should be unreachable as we check if the payload can be
        // pushed in the loop condition.
        frame.push_pdu(
            Command::fprd(sd.configured_address(), RegisterAddress::AlStatus.into()).into(),
            (),
            Some(AlControl::PACKED_LEN as u16),
        )?;

        num_in_this_frame += 1;

        // A status check datagram is 14 bytes, meaning we can fit at most just over 100
        // checks per normal EtherCAT frame. This leaves spare PDU indices available for
        // other purposes, however if the user is using jumbo frames or something, we should
        // always leave some indices free for e.g. other threads.
        if num_in_this_frame > 128 {
            break;
        }
    }

    fmt::trace!(
        "--> Pushed {} status checks into frame {}",
        num_in_this_frame,
        frame.storage_slot_index()
    );

    Ok((subdevices, num_in_this_frame))
}

// Methods for any state where a PDI has been configured.
impl<const MAX_SUBDEVICES: usize, const MAX_PDI: usize, S, DC>
    SubDeviceGroup<MAX_SUBDEVICES, MAX_PDI, S, DC>
where
    S: HasPdi,
{
    /// Borrow an individual SubDevice.
    #[doc(alias = "slave")]
    pub fn subdevice<'maindevice, 'group>(
        &'group self,
        maindevice: &'maindevice MainDevice<'maindevice>,
        index: usize,
    ) -> Result<SubDeviceRef<'maindevice, SubDevicePdi<'group, MAX_PDI>>, Error> {
        let subdevice = self.inner().subdevices.get(index).ok_or(Error::NotFound {
            item: Item::SubDevice,
            index: Some(index),
        })?;

        let io_ranges = subdevice.io_segments().clone();

        let IoRanges {
            input: input_range,
            output: output_range,
        } = &io_ranges;

        fmt::trace!(
            "Get SubDevice {:#06x} IO ranges I: {}, O: {} (group PDI {} byte subset of {} byte max)",
            subdevice.configured_address(),
            input_range,
            output_range,
            self.pdi_len,
            MAX_PDI
        );

        Ok(SubDeviceRef::new(
            maindevice,
            subdevice.configured_address(),
            SubDevicePdi::new(subdevice, &self.pdi),
        ))
    }

    /// Borrow an individual SubDevice, found by configured address.
    pub fn subdevice_by_configured_address<'maindevice, 'group>(
        &'group self,
        maindevice: &'maindevice MainDevice<'maindevice>,
        configured_address: u16,
    ) -> Result<SubDeviceRef<'maindevice, SubDevicePdi<'group, MAX_PDI>>, Error> {
        let subdevice = self
            .inner()
            .subdevices
            .iter()
            .find(|sd| sd.configured_address() == configured_address)
            .ok_or(Error::NotFound {
                item: Item::SubDevice,
                index: Some(usize::from(configured_address)),
            })?;

        let io_ranges = subdevice.io_segments().clone();

        let IoRanges {
            input: input_range,
            output: output_range,
        } = &io_ranges;

        fmt::trace!(
            "Get SubDevice {:#06x} IO ranges I: {}, O: {} (group PDI {} byte subset of {} byte max)",
            subdevice.configured_address(),
            input_range,
            output_range,
            self.pdi_len,
            MAX_PDI
        );

        Ok(SubDeviceRef::new(
            maindevice,
            subdevice.configured_address(),
            SubDevicePdi::new(subdevice, &self.pdi),
        ))
    }

    /// Get an iterator over all SubDevices in this group.
    pub fn iter<'group, 'maindevice>(
        &'group self,
        maindevice: &'maindevice MainDevice<'maindevice>,
    ) -> impl Iterator<Item = SubDeviceRef<'group, SubDevicePdi<'group, MAX_PDI>>>
    where
        'maindevice: 'group,
    {
        self.inner().subdevices.iter().map(|sd| {
            SubDeviceRef::new(
                maindevice,
                sd.configured_address,
                SubDevicePdi::new(sd, &self.pdi),
            )
        })
    }

    /// Drive the SubDevice group's inputs and outputs.
    ///
    /// A `SubDeviceGroup` will not process any inputs or outputs unless this method is called
    /// periodically. It will send an `LRW` to update SubDevice outputs and read SubDevice inputs.
    ///
    /// This method returns a [`TxRxResponse`] containing the working counter and a list of all
    /// SubDevice states on success.
    ///
    /// # Errors
    ///
    /// This method will return with an error if the PDU could not be sent over the network, or the
    /// response times out.
    pub async fn tx_rx<'sto>(
        &self,
        maindevice: &'sto MainDevice<'sto>,
    ) -> Result<TxRxResponse<MAX_SUBDEVICES>, Error> {
        fmt::trace!(
            "Group TX/RX, start address {:#010x}, data len {}, of which read bytes: {}",
            self.inner().pdi_start.start_address,
            self.pdi_len,
            self.read_pdi_len
        );

        let mut pdi_lock = self.pdi.write();

        let mut total_bytes_sent = 0;
        let mut lrw_wkc_sum = 0;

        let mut subdevices = self.inner().subdevices.iter();
        let mut total_checks = 0;
        let mut subdevice_states = heapless::Vec::<_, MAX_SUBDEVICES>::new();

        loop {
            let chunk_len = self.pdi_len.saturating_sub(total_bytes_sent);

            if chunk_len == 0 && total_checks >= self.len() {
                break;
            }

            let chunk_start = total_bytes_sent.min(self.pdi_len);
            let chunk = pdi_lock.get_mut()[chunk_start..(chunk_start + chunk_len)].as_ref();

            let mut frame = maindevice.pdu_loop.alloc_frame()?;

            // Start offset in the EtherCAT address space
            let pushed_chunk = if !chunk.is_empty() {
                let start_addr = self.inner().pdi_start.start_address + total_bytes_sent as u32;

                frame.push_pdu_slice_rest(Command::lrw(start_addr).into(), chunk)?
            } else {
                None
            };

            // If there's space left, push as many state checks as we can into the frame
            let (rest, num_checks_in_this_frame) = push_state_checks(subdevices, &mut frame)?;
            subdevices = rest;
            total_checks += num_checks_in_this_frame;

            if frame.is_empty() {
                break;
            }

            let frame = frame.mark_sendable(
                &maindevice.pdu_loop,
                maindevice.timeouts.pdu,
                maindevice.config.retry_behaviour.retry_count(),
            );

            maindevice.pdu_loop.wake_sender();

            let received = frame.await?;

            let mut pdus = received.into_pdu_iter();

            // If we pushed a non-zero amount of PDI bytes, process the response
            if let Some((bytes_in_this_chunk, _pdu_handle)) = pushed_chunk {
                let wkc = self.process_received_pdi_chunk(
                    total_bytes_sent,
                    bytes_in_this_chunk,
                    &pdus.next().ok_or(Error::Internal)??,
                    &mut pdi_lock,
                )?;

                total_bytes_sent += bytes_in_this_chunk;
                lrw_wkc_sum += wkc;
            }

            // If there are any more PDUs, these are state checks
            for state_check_pdu in pdus {
                let state_check_pdu = state_check_pdu?;

                let state = AlControl::unpack_from_slice(&state_check_pdu)?;

                let _ = subdevice_states.push(state.state);
            }
        }

        Ok(TxRxResponse {
            working_counter: lrw_wkc_sum,
            subdevice_states,
            extra: (),
        })
    }

    /// Drive the SubDevice group's inputs and outputs and synchronise EtherCAT system time with
    /// `FRMW`.
    ///
    /// A `SubDeviceGroup` will not process any inputs or outputs unless this method is called
    /// periodically. It will send an `LRW` to update SubDevice outputs and read SubDevice inputs.
    ///
    /// This method returns a [`TxRxResponse`] struct, containing the working counter, group
    /// SubDevice statuses and the current EtherCAT system time in nanoseconds on success. If the
    /// PDI must be sent in multiple chunks, the returned working counter is the sum of all returned
    /// working counter values.
    ///
    /// # Errors
    ///
    /// This method will return with an error if the PDU could not be sent over the network, or the
    /// response times out.
    pub async fn tx_rx_sync_system_time<'sto>(
        &self,
        maindevice: &'sto MainDevice<'sto>,
    ) -> Result<TxRxResponse<MAX_SUBDEVICES, Option<u64>>, Error> {
        let mut pdi_lock = self.pdi.write();

        fmt::trace!(
            "Group TX/RX with DC sync, start address {:#010x}, data len {}, of which read bytes: {}",
            self.inner().pdi_start.start_address,
            self.pdi_len,
            self.read_pdi_len
        );

        if let Some(dc_ref) = maindevice.dc_ref_address() {
            let mut total_bytes_sent = 0;
            let mut time = 0;
            let mut lrw_wkc_sum = 0;
            let mut time_read = false;

            let mut subdevices = self.inner().subdevices.iter();
            let mut total_checks = 0;
            let mut subdevice_states = heapless::Vec::<_, MAX_SUBDEVICES>::new();

            loop {
                let mut frame = maindevice.pdu_loop.alloc_frame()?;

                let dc_handle = if !time_read {
                    let dc_handle = frame.push_pdu(
                        Command::frmw(dc_ref, RegisterAddress::DcSystemTime.into()).into(),
                        0u64,
                        None,
                    )?;

                    // Just double checking
                    debug_assert_eq!(dc_handle.alloc_size, DC_PDU_SIZE);

                    Some(dc_handle)
                } else {
                    None
                };

                let chunk_start = total_bytes_sent.min(self.pdi_len);
                let chunk_len = self.pdi_len.saturating_sub(total_bytes_sent);
                let chunk = pdi_lock.get_mut()[chunk_start..(chunk_start + chunk_len)].as_ref();

                let pushed_chunk = if !chunk.is_empty() {
                    let start_addr = self.inner().pdi_start.start_address + total_bytes_sent as u32;

                    frame.push_pdu_slice_rest(Command::lrw(start_addr).into(), chunk)?
                } else {
                    None
                };

                if let Some((bytes_in_this_chunk, _)) = pushed_chunk {
                    fmt::trace!("Wrote {} byte chunk", bytes_in_this_chunk);
                }

                // If there's space left, push as many state checks as we can into the frame
                let (rest, num_checks_in_this_frame) = push_state_checks(subdevices, &mut frame)?;
                subdevices = rest;
                total_checks += num_checks_in_this_frame;

                if frame.is_empty() {
                    break Ok(TxRxResponse {
                        working_counter: lrw_wkc_sum,
                        subdevice_states,
                        extra: Some(time),
                    });
                }

                let frame = frame.mark_sendable(
                    &maindevice.pdu_loop,
                    maindevice.timeouts.pdu,
                    maindevice.config.retry_behaviour.retry_count(),
                );

                maindevice.pdu_loop.wake_sender();

                let received = frame.await?;

                let mut pdus = received.into_pdu_iter();

                if dc_handle.is_some() {
                    let dc_pdu = pdus.next().ok_or(Error::Internal)?;

                    time =
                        dc_pdu.and_then(|rx| u64::unpack_from_slice(&rx).map_err(Error::from))?;

                    time_read = true;
                }

                // If we pushed a non-zero amount of PDI bytes, process the response
                if let Some((bytes_in_this_chunk, _pdu_handle)) = pushed_chunk {
                    let wkc = self.process_received_pdi_chunk(
                        total_bytes_sent,
                        bytes_in_this_chunk,
                        &pdus.next().ok_or(Error::Internal)??,
                        &mut pdi_lock,
                    )?;

                    total_bytes_sent += bytes_in_this_chunk;
                    lrw_wkc_sum += wkc;
                }

                // If there are any more PDUs, these are state checks
                for state_check_pdu in pdus {
                    let state_check_pdu = state_check_pdu?;

                    let state = AlControl::unpack_from_slice(&state_check_pdu)?;

                    let _ = subdevice_states.push(state.state);
                }

                // NOTE: Not using a while loop as we want to always send the DC sync PDU even if
                // the PDI is empty.
                if chunk_len == 0 && total_checks >= self.len() {
                    break Ok(TxRxResponse {
                        working_counter: lrw_wkc_sum,
                        subdevice_states,
                        extra: Some(time),
                    });
                }
            }
        } else {
            self.tx_rx(maindevice).await.map(|response| TxRxResponse {
                working_counter: response.working_counter,
                subdevice_states: response.subdevice_states,
                extra: None,
            })
        }
    }

    fn process_received_pdi_chunk(
        &self,
        total_bytes_sent: usize,
        bytes_in_this_chunk: usize,
        data: &ReceivedPdu<'_>,
        pdi_lock: &mut spin::rwlock::RwLockWriteGuard<
            '_,
            MySyncUnsafeCell<[u8; MAX_PDI]>,
            crate::SpinStrategy,
        >,
    ) -> Result<u16, Error> {
        let wkc = data.working_counter;

        let rx_range = total_bytes_sent.min(self.read_pdi_len)
            ..(total_bytes_sent + bytes_in_this_chunk).min(self.read_pdi_len);

        let inputs_chunk = &mut pdi_lock.get_mut()[rx_range];

        inputs_chunk.copy_from_slice(data.get(0..inputs_chunk.len()).ok_or(Error::Internal)?);

        Ok(wkc)
    }
}

// Methods for when the group has a PDI AND has Distributed Clocks configured
impl<const MAX_SUBDEVICES: usize, const MAX_PDI: usize, S>
    SubDeviceGroup<MAX_SUBDEVICES, MAX_PDI, S, HasDc>
where
    S: HasPdi,
{
    /// Drive the SubDevice group's inputs and outputs, synchronise EtherCAT system time with
    /// `FRMW`, and return cycle timing and SubDevice state information.
    ///
    /// A `SubDeviceGroup` will not process any inputs or outputs unless this method is called
    /// periodically. It will send an `LRW` to update SubDevice outputs and read SubDevice inputs.
    ///
    /// This method returns a [`TxRxResponse`] struct, containing the working counter, a
    /// [`CycleInfo`] containing values that can be used to synchronise the MainDevice to the
    /// network SYNC0 event, and the state of all SubDevices in the group.
    ///
    /// # Errors
    ///
    /// This method will return with an error if the PDU could not be sent over the network, or the
    /// response times out.
    ///
    /// # Examples
    ///
    /// This example sends process data at 2.5ms offset into a 5ms cycle.
    ///
    /// ```rust,no_run
    /// # use ethercrab::{
    /// #     error::Error,
    /// #     subdevice_group::{CycleInfo, DcConfiguration, TxRxResponse},
    /// #     std::ethercat_now,
    /// #     MainDevice, MainDeviceConfig, PduStorage, Timeouts, DcSync,
    /// # };
    /// # use std::time::{Duration, Instant};
    /// # const MAX_SUBDEVICES: usize = 16;
    /// # const MAX_PDU_DATA: usize = PduStorage::element_size(1100);
    /// # const MAX_FRAMES: usize = 32;
    /// # const PDI_LEN: usize = 64;
    /// # static PDU_STORAGE: PduStorage<MAX_FRAMES, MAX_PDU_DATA> = PduStorage::new();
    /// # fn main() -> Result<(), Error> { smol::block_on(async {
    /// let (_tx, _rx, pdu_loop) = PDU_STORAGE.try_split().expect("can only split once");
    ///
    /// let maindevice = MainDevice::new(pdu_loop, Timeouts::default(), MainDeviceConfig::default());
    ///
    /// let cycle_time = Duration::from_millis(5);
    ///
    /// let mut group = maindevice
    ///     .init_single_group::<MAX_SUBDEVICES, PDI_LEN>(ethercat_now)
    ///     .await
    ///     .expect("Init");
    ///
    /// // This example enables SYNC0 for every detected SubDevice
    /// for mut subdevice in group.iter_mut(&maindevice) {
    ///     subdevice.set_dc_sync(DcSync::Sync0);
    /// }
    ///
    /// let mut group = group
    ///     .into_pre_op_pdi(&maindevice)
    ///     .await
    ///     .expect("PRE-OP -> PRE-OP with PDI")
    ///     .configure_dc_sync(
    ///         &maindevice,
    ///         DcConfiguration {
    ///             // Start SYNC0 100ms in the future
    ///             start_delay: Duration::from_millis(100),
    ///             // SYNC0 period should be the same as the process data loop in most cases
    ///             sync0_period: cycle_time,
    ///             // Send process data half way through cycle
    ///             sync0_shift: cycle_time / 2,
    ///         },
    ///     )
    ///     .await
    ///     .expect("DC configuration")
    ///     .request_into_op(&maindevice)
    ///     .await
    ///     .expect("PRE-OP -> SAFE-OP -> OP");
    ///
    /// // Wait for all SubDevices in the group to reach OP, whilst sending PDI to allow DC to start
    /// // correctly.
    /// loop {
    ///     let now = Instant::now();
    ///
    ///     let response @ TxRxResponse {
    ///         working_counter: _wkc,
    ///         extra: CycleInfo {
    ///             next_cycle_wait, ..
    ///         },
    ///         ..
    ///     } = group.tx_rx_dc(&maindevice).await.expect("TX/RX");
    ///
    ///     if response.all_op() {
    ///         break;
    ///     }
    ///
    ///     smol::Timer::at(now + next_cycle_wait).await;
    /// }
    ///
    /// // Main application process data cycle
    /// loop {
    ///     let now = Instant::now();
    ///
    ///     let TxRxResponse {
    ///         working_counter: _wkc,
    ///         extra: CycleInfo {
    ///             next_cycle_wait, ..
    ///         },
    ///         ..
    ///     } = group.tx_rx_dc(&maindevice).await.expect("TX/RX");
    ///
    ///     // Process data computations happen here
    ///
    ///     smol::Timer::at(now + next_cycle_wait).await;
    /// }
    /// # }) }
    /// ```
    pub async fn tx_rx_dc<'sto>(
        &self,
        maindevice: &'sto MainDevice<'sto>,
    ) -> Result<TxRxResponse<MAX_SUBDEVICES, CycleInfo>, Error> {
        fmt::trace!(
            "Group TX/RX with DC sync, start address {:#010x}, data len {}, of which read bytes: {}",
            self.inner().pdi_start.start_address,
            self.pdi_len,
            self.read_pdi_len
        );

        let mut pdi_lock = self.pdi.write();

        let mut total_bytes_sent = 0;
        let mut time = 0;
        let mut lrw_wkc_sum = 0;
        let mut time_read = false;

        let mut subdevices = self.inner().subdevices.iter();
        let mut total_checks = 0;
        let mut subdevice_states = heapless::Vec::<_, MAX_SUBDEVICES>::new();

        loop {
            let mut frame = maindevice.pdu_loop.alloc_frame()?;

            let dc_handle = if !time_read {
                let dc_handle = frame.push_pdu(
                    Command::frmw(self.dc_conf.reference, RegisterAddress::DcSystemTime.into())
                        .into(),
                    0u64,
                    None,
                )?;

                // Just double checking
                debug_assert_eq!(dc_handle.alloc_size, DC_PDU_SIZE);

                Some(dc_handle)
            } else {
                None
            };

            let chunk_start = total_bytes_sent.min(self.pdi_len);
            let chunk_len = self.pdi_len.saturating_sub(total_bytes_sent);
            let chunk = pdi_lock.get_mut()[chunk_start..(chunk_start + chunk_len)].as_ref();

            let pushed_chunk = if !chunk.is_empty() {
                let start_addr = self.inner().pdi_start.start_address + total_bytes_sent as u32;

                frame.push_pdu_slice_rest(Command::lrw(start_addr).into(), chunk)?
            } else {
                None
            };

            // If there's space left, push as many state checks as we can into the frame
            let (rest, num_checks_in_this_frame) = push_state_checks(subdevices, &mut frame)?;
            subdevices = rest;
            total_checks += num_checks_in_this_frame;

            if frame.is_empty() {
                break;
            }

            let frame = frame.mark_sendable(
                &maindevice.pdu_loop,
                maindevice.timeouts.pdu,
                maindevice.config.retry_behaviour.retry_count(),
            );

            maindevice.pdu_loop.wake_sender();

            let received = frame.await?;

            let mut pdus = received.into_pdu_iter();

            if dc_handle.is_some() {
                let dc_pdu = pdus.next().ok_or(Error::Internal)?;

                time = dc_pdu.and_then(|rx| u64::unpack_from_slice(&rx).map_err(Error::from))?;

                time_read = true;
            }

            // If we pushed a non-zero amount of PDI bytes, process the response
            if let Some((bytes_in_this_chunk, _pdu_handle)) = pushed_chunk {
                let wkc = self.process_received_pdi_chunk(
                    total_bytes_sent,
                    bytes_in_this_chunk,
                    &pdus.next().ok_or(Error::Internal)??,
                    &mut pdi_lock,
                )?;

                total_bytes_sent += bytes_in_this_chunk;
                lrw_wkc_sum += wkc;
            }

            // If there are any more PDUs, these are state checks
            for state_check_pdu in pdus {
                let state_check_pdu = state_check_pdu?;

                let state = AlControl::unpack_from_slice(&state_check_pdu)?;

                let _ = subdevice_states.push(state.state);
            }

            // NOTE: Not using a while loop as we want to always send the DC sync PDU even if the
            // PDI is empty.
            // This condition will exit the loop if the whole PDI has been sent as well as all
            // SubDevice status check PDUs.
            if chunk_len == 0 && total_checks >= self.len() {
                break;
            }
        }

        // Nanoseconds from the start of the cycle. This works because the first SYNC0 pulse
        // time is rounded to a whole number of `sync0_period`-length cycles.
        let cycle_start_offset = time % self.dc_conf.sync0_period;

        let time_to_next_iter =
            (self.dc_conf.sync0_period - cycle_start_offset) + self.dc_conf.sync0_shift;

        Ok(TxRxResponse {
            working_counter: lrw_wkc_sum,
            subdevice_states,
            extra: CycleInfo {
                dc_system_time: time,
                cycle_start_offset: Duration::from_nanos(cycle_start_offset),
                next_cycle_wait: Duration::from_nanos(time_to_next_iter),
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        MainDeviceConfig, PduStorage, Timeouts,
        ethernet::{EthernetAddress, EthernetFrame},
        pdu_loop::ReceivedFrame,
    };
    use core::sync::atomic::{AtomicBool, AtomicU8, Ordering};
    use std::{sync::Arc, thread};

    #[tokio::test(flavor = "multi_thread", worker_threads = 3)]
    async fn tx_rx_miri() {
        const MAX_SUBDEVICES: usize = 16;
        const MAX_PDU_DATA: usize = PduStorage::element_size(8);
        const MAX_FRAMES: usize = 128;
        const MAX_PDI: usize = 128;

        static PDU_STORAGE: PduStorage<MAX_FRAMES, MAX_PDU_DATA> = PduStorage::new();

        crate::test_logger();

        let (mock_net_tx, mock_net_rx) = std::sync::mpsc::sync_channel::<Vec<u8>>(16);

        let (mut tx, mut rx, pdu_loop) = PDU_STORAGE.try_split().expect("can only split once");

        let stop = Arc::new(AtomicBool::new(false));

        let stop1 = stop.clone();

        let tx_handle = thread::spawn(move || {
            fmt::info!("Spawn TX task");

            while !stop1.load(Ordering::Relaxed) {
                while let Some(frame) = tx.next_sendable_frame() {
                    fmt::info!("Sendable frame");

                    frame
                        .send_blocking(|bytes| {
                            mock_net_tx.send(bytes.to_vec()).unwrap();

                            Ok(bytes.len())
                        })
                        .unwrap();

                    thread::yield_now();
                }

                thread::sleep(Duration::from_millis(1));
            }
        });

        let stop1 = stop.clone();

        let rx_handle = thread::spawn(move || {
            fmt::info!("Spawn RX task");

            while let Ok(ethernet_frame) = mock_net_rx.recv() {
                fmt::info!("RX task received packet");

                // Let frame settle for a mo
                thread::sleep(Duration::from_millis(1));

                // Munge fake sent frame into a fake received frame
                let ethernet_frame = {
                    let mut frame = EthernetFrame::new_checked(ethernet_frame).unwrap();
                    frame.set_src_addr(EthernetAddress([0x12, 0x10, 0x10, 0x10, 0x10, 0x10]));
                    frame.into_inner()
                };

                while rx.receive_frame(&ethernet_frame).is_err() {}

                thread::yield_now();

                if stop1.load(Ordering::Relaxed) {
                    break;
                }
            }
        });

        let maindevice = Arc::new(MainDevice::new(
            pdu_loop,
            Timeouts {
                pdu: Duration::from_secs(1),
                wait_loop_delay: Duration::ZERO,
                ..Timeouts::default()
            },
            MainDeviceConfig::default(),
        ));

        let group: SubDeviceGroup<MAX_SUBDEVICES, MAX_PDI, PreOpPdi, NoDc> = SubDeviceGroup {
            id: GroupId(0),
            pdi: spin::rwlock::RwLock::new(MySyncUnsafeCell::new([0u8; MAX_PDI])),
            read_pdi_len: 32,
            pdi_len: 96,
            inner: MySyncUnsafeCell::new(GroupInner {
                subdevices: heapless::Vec::new(),
                pdi_start: PdiOffset::default(),
            }),
            dc_conf: NoDc,
            _state: PhantomData,
        };

        let out = group.tx_rx(&maindevice).await;

        // No subdevices so no WKC, but success
        assert_eq!(
            out,
            Ok(TxRxResponse {
                working_counter: 0,
                subdevice_states: heapless::Vec::new(),
                extra: ()
            })
        );

        stop.store(true, Ordering::Relaxed);

        tx_handle.join().unwrap();
        rx_handle.join().unwrap();
    }

    #[test]
    fn multi_state_checks_single_frame() {
        const MAX_FRAMES: usize = 1;
        const MAX_PDU_DATA: usize = PduStorage::element_size(AlControl::PACKED_LEN);
        static PDU_STORAGE: PduStorage<MAX_FRAMES, MAX_PDU_DATA> = PduStorage::new();

        crate::test_logger();

        let (_tx, _rx, pdu_loop) = PDU_STORAGE.try_split().expect("can only split once");

        let mut frame = pdu_loop.alloc_frame().expect("No frame");

        assert!(
            frame.can_push_pdu_payload(AlControl::PACKED_LEN),
            "should be possible to push one status check PDU"
        );
        assert!(
            !frame.can_push_pdu_payload(AlControl::PACKED_LEN + 12),
            "test requires the frame to fit exactly one status check PDU"
        );

        let single_sd = vec![SubDevice {
            ..SubDevice::default()
        }];

        let subdevices = single_sd.iter();

        let (rest, num_pushed) =
            push_state_checks(subdevices, &mut frame).expect("Could not push status check");

        assert_eq!(rest.count(), 0);
        assert_eq!(num_pushed, single_sd.len());

        assert!(!frame.can_push_pdu_payload(1), "frame should be full");
    }

    #[test]
    fn multi_state_checks_space_left_over() {
        // 1 byte left. AlControl takes 2 bytes.
        const SPACE_LEFT: usize = 1;

        const MAX_FRAMES: usize = 1;
        const MAX_PDU_DATA: usize = (AlControl::PACKED_LEN + CreatedFrame::PDU_OVERHEAD_BYTES) * 2
            + (SPACE_LEFT + CreatedFrame::PDU_OVERHEAD_BYTES)
            // Ethernet and EtherCAT frame headers
            + 16;
        static PDU_STORAGE: PduStorage<MAX_FRAMES, MAX_PDU_DATA> = PduStorage::new();

        crate::test_logger();

        let (_tx, _rx, pdu_loop) = PDU_STORAGE.try_split().expect("can only split once");

        let mut frame = pdu_loop.alloc_frame().expect("No frame");

        let sds = vec![
            SubDevice {
                ..SubDevice::default()
            },
            SubDevice {
                ..SubDevice::default()
            },
            SubDevice {
                ..SubDevice::default()
            },
        ];

        let subdevices = sds.iter();

        let (rest, num_pushed) =
            push_state_checks(subdevices, &mut frame).expect("Could not push status check");

        assert_eq!(num_pushed, 2, "frame should hold two SD status checks");
        assert_eq!(rest.count(), 1, "frame can only hold two SD status checks");

        assert!(
            frame.can_push_pdu_payload(SPACE_LEFT),
            "frame has {} bytes available",
            SPACE_LEFT
        );
    }

    // This records the behaviour of a DC setup of the following 16 SubDevices:
    //
    // - EK1100
    // - EL2828
    // - EL2889
    // - EL2004
    // - EL1004
    // - EL1018
    // - EL1008
    // - EL1004
    // - EL2004
    // - EL2008
    // - EL1008
    // - EL2008
    // - EL2008
    // - EL2522
    // - EL1258
    // - EL9505
    #[test]
    fn large_group_frame_split() {
        const MAX_SUBDEVICES: usize = 32;
        const MAX_PDU_DATA: usize = PduStorage::element_size(256);
        const MAX_FRAMES: usize = 32;
        const MAX_PDI: usize = 512;
        static PDU_STORAGE: PduStorage<MAX_FRAMES, MAX_PDU_DATA> = PduStorage::new();

        crate::test_logger();

        let (mock_net_tx, mock_net_rx) = std::sync::mpsc::sync_channel::<Vec<u8>>(16);

        let (mut tx, mut rx, pdu_loop) = PDU_STORAGE.try_split().expect("can only split once");

        let maindevice = Arc::new(MainDevice::new(
            pdu_loop,
            Timeouts::default(),
            MainDeviceConfig::default(),
        ));

        let stop = Arc::new(AtomicBool::new(false));

        let stop1 = stop.clone();

        let tx_handle = thread::spawn(move || {
            fmt::info!("Spawn TX task");

            while !stop1.load(Ordering::Relaxed) {
                while let Some(frame) = tx.next_sendable_frame() {
                    fmt::info!("Sendable frame");

                    frame
                        .send_blocking(|bytes| {
                            mock_net_tx.send(bytes.to_vec()).unwrap();

                            Ok(bytes.len())
                        })
                        .unwrap();

                    thread::yield_now();
                }
            }
        });

        let stop1 = stop.clone();

        let rx_handle = thread::spawn(move || {
            fmt::info!("Spawn RX task");

            while let Ok(ethernet_frame) = mock_net_rx.recv() {
                fmt::info!("RX task received packet");

                // Munge fake sent frame into a fake received frame
                let ethernet_frame = {
                    let mut frame = EthernetFrame::new_checked(ethernet_frame).unwrap();
                    frame.set_src_addr(EthernetAddress([0x12, 0x10, 0x10, 0x10, 0x10, 0x10]));
                    frame.into_inner()
                };

                while rx.receive_frame(&ethernet_frame).is_err() {}

                thread::yield_now();

                if stop1.load(Ordering::Relaxed) {
                    break;
                }
            }
        });

        fn sd(addr: u16) -> SubDevice {
            SubDevice {
                configured_address: addr,
                ..SubDevice::default()
            }
        }

        let subdevices = heapless::Vec::<_, MAX_SUBDEVICES>::from_slice(&[
            sd(0x1000),
            sd(0x1001),
            sd(0x1002),
            sd(0x1003),
            sd(0x1004),
            sd(0x1005),
            sd(0x1006),
            sd(0x1007),
            sd(0x1008),
            sd(0x1009),
            sd(0x100a),
            sd(0x100b),
            sd(0x100c),
            sd(0x100d),
            sd(0x100e),
            sd(0x100f),
        ])
        .unwrap();

        // Test setup had 16 devices
        assert_eq!(subdevices.len(), 16);

        let group = SubDeviceGroup {
            id: GroupId(0),
            pdi: spin::rwlock::RwLock::new(MySyncUnsafeCell::new([0u8; MAX_PDI])),
            read_pdi_len: 406,
            pdi_len: 474,
            inner: MySyncUnsafeCell::new(GroupInner {
                subdevices,
                pdi_start: PdiOffset { start_address: 0 },
            }),
            dc_conf: HasDc {
                sync0_period: 100_000,
                sync0_shift: 0,
                reference: 0,
            },
            _state: PhantomData::<Op>,
        };

        cassette::block_on(group.tx_rx_dc(&maindevice)).unwrap();

        stop.store(true, Ordering::Relaxed);

        tx_handle.join().unwrap();
        rx_handle.join().unwrap();

        const PDI_FRAME_0: usize = 236;
        const PDI_FRAME_1: usize = 238;

        assert_eq!(PDI_FRAME_0 + PDI_FRAME_1, 474);

        // Expected PDU lengths for each frame
        let expected_pdus = [
            [
                8,           // DC FRMW
                PDI_FRAME_0, // Consume rest of frame with PDI
            ]
            .as_slice(),
            &[
                PDI_FRAME_1, // Entire frame filled with PDI
                2,           // First status check
            ],
            // 15 remaining SubDevice status checks
            &[2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2],
        ];

        // We should have sent 3 frames in this test
        for (i, expected_lens) in expected_pdus.iter().enumerate() {
            let f = maindevice
                .pdu_loop
                .test_only_storage_ref()
                .frame_at_index(i);

            let idx = AtomicU8::new(i as u8);

            let b = ReceivedFrame::from_frame_element_for_test_only(f, &idx, MAX_PDU_DATA);

            let expected_pdu_count = expected_lens.len();
            let mut actual_pdu_count = 0;

            for (pdu_idx, pdu) in b.into_pdu_iter().enumerate() {
                let pdu = pdu.unwrap();

                actual_pdu_count += 1;

                assert_eq!(
                    pdu.len(),
                    expected_lens[pdu_idx],
                    "frame {}, PDU {} length",
                    i,
                    pdu_idx
                );
            }

            assert_eq!(
                actual_pdu_count, expected_pdu_count,
                "frame {} PDU count",
                i
            );
        }

        let f = maindevice
            .pdu_loop
            .test_only_storage_ref()
            .frame_at_index(3);
        let idx = AtomicU8::new(3);
        let b = ReceivedFrame::from_frame_element_for_test_only(f, &idx, MAX_PDU_DATA);

        // 4th frame should be empty as we only sent 3
        assert_eq!(b.into_pdu_iter().count(), 0);
    }
}
