//! A group of SubDevices.
//!
//! SubDevices can be divided into multiple groups to allow multiple tasks to run concurrently,
//! potentially at different tick rates.

mod group_id;
mod handle;
mod sync_unsafe_cell;
#[cfg(test)]
mod tests;
mod tx_rx_impl;
mod tx_rx_response;

use crate::{
    DcSync, MainDevice, RegisterAddress, SubDeviceState,
    al_control::AlControl,
    command::Command,
    error::{DistributedClockError, Error, Item},
    fmt,
    pdi::PdiOffset,
    pdu_loop::CreatedFrame,
    subdevice::{
        IoRanges, SubDevice, SubDeviceRef, configuration::PdoDirection, pdi::SubDevicePdi,
    },
    timer_factory::IntoTimeout,
};
use core::{marker::PhantomData, sync::atomic::AtomicUsize, time::Duration};
use ethercrab_wire::{EtherCrabWireRead, EtherCrabWireSized};
use lock_api::{RawRwLock, RwLock};

pub use self::group_id::GroupId;
pub use self::handle::SubDeviceGroupHandle;
pub use self::tx_rx_response::TxRxResponse;
pub(crate) use sync_unsafe_cell::MySyncUnsafeCell;

static GROUP_ID: AtomicUsize = AtomicUsize::new(0);

/// The size of a DC sync PDU.
const DC_PDU_SIZE: usize = CreatedFrame::PDU_OVERHEAD_BYTES + u64::PACKED_LEN;

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
pub struct SubDeviceGroup<
    const MAX_SUBDEVICES: usize,
    const MAX_PDI: usize,
    R: RawRwLock = crate::DefaultLock,
    S = PreOp,
    DC = NoDc,
> {
    id: GroupId,
    pdi: RwLock<R, MySyncUnsafeCell<[u8; MAX_PDI]>>,
    /// The number of bytes at the beginning of the PDI reserved for SubDevice inputs.
    read_pdi_len: usize,
    /// The total length (I and O) of the PDI for this group.
    pdi_len: usize,
    inner: MySyncUnsafeCell<GroupInner<MAX_SUBDEVICES>>,
    dc_conf: DC,
    _state: PhantomData<S>,
}

impl<const MAX_SUBDEVICES: usize, const MAX_PDI: usize, R: RawRwLock, DC>
    SubDeviceGroup<MAX_SUBDEVICES, MAX_PDI, R, PreOp, DC>
{
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
                    PdoDirection::MasterRead,
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
                    PdoDirection::MasterWrite,
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

    /// Transition the group from PRE-OP -> SAFE-OP -> OP.
    ///
    /// To transition individually from PRE-OP to SAFE-OP, then SAFE-OP to OP, see
    /// [`SubDeviceGroup::into_safe_op`].
    pub async fn into_op(
        self,
        maindevice: &MainDevice<'_>,
    ) -> Result<SubDeviceGroup<MAX_SUBDEVICES, MAX_PDI, R, Op, DC>, Error> {
        let self_ = self.into_safe_op(maindevice).await?;

        self_.into_op(maindevice).await
    }

    /// Configure FMMUs, but leave the group in [`PreOp`] state.
    ///
    /// This method is used to obtain access to the group's PDI and related functionality. All SDO
    /// and other configuration should be complete at this point otherwise issues with cyclic data
    /// may occur (e.g. incorrect lengths, misplaced fields, etc).
    pub async fn into_pre_op_pdi(
        mut self,
        maindevice: &MainDevice<'_>,
    ) -> Result<SubDeviceGroup<MAX_SUBDEVICES, MAX_PDI, R, PreOpPdi, DC>, Error> {
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
    ) -> Result<SubDeviceGroup<MAX_SUBDEVICES, MAX_PDI, R, SafeOp, DC>, Error> {
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
    ) -> Result<SubDeviceGroup<MAX_SUBDEVICES, MAX_PDI, R, Init, DC>, Error> {
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

impl<const MAX_SUBDEVICES: usize, const MAX_PDI: usize, R: RawRwLock, S, DC>
    SubDeviceGroup<MAX_SUBDEVICES, MAX_PDI, R, S, DC>
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
    ) -> Result<SubDeviceGroup<MAX_SUBDEVICES, MAX_PDI, R, PreOpPdi, HasDc>, Error> {
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
                .send(maindevice, sync0_period as u32)
                .await?;

            let flags = if let DcSync::Sync01 { sync1_period } = subdevice.dc_sync() {
                let sync1_period = u64::try_from(sync1_period.as_nanos())?;

                subdevice
                    .write(RegisterAddress::DcSync1CycleTime)
                    .send(maindevice, sync1_period as u32)
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
                sync0_period,
                sync0_shift: sync0_shift.as_nanos() as u64,
                reference,
            },
            _state: PhantomData,
        })
    }
}

impl<const MAX_SUBDEVICES: usize, const MAX_PDI: usize, R: RawRwLock, DC>
    SubDeviceGroup<MAX_SUBDEVICES, MAX_PDI, R, PreOpPdi, DC>
{
    /// Transition the SubDevice group from PRE-OP to SAFE-OP.
    pub async fn into_safe_op(
        self,
        maindevice: &MainDevice<'_>,
    ) -> Result<SubDeviceGroup<MAX_SUBDEVICES, MAX_PDI, R, SafeOp, DC>, Error> {
        self.transition_to(maindevice, SubDeviceState::SafeOp).await
    }

    /// Transition all SubDevices in the group from PRE-OP to SAFE-OP, then to OP.
    ///
    /// This is a convenience method that calls [`into_safe_op`](SubDeviceGroup::into_safe_op) then
    /// [`into_op`](SubDeviceGroup::into_op).
    pub async fn into_op(
        self,
        maindevice: &MainDevice<'_>,
    ) -> Result<SubDeviceGroup<MAX_SUBDEVICES, MAX_PDI, R, Op, DC>, Error> {
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
    ) -> Result<SubDeviceGroup<MAX_SUBDEVICES, MAX_PDI, R, Op, DC>, Error> {
        let self_ = self.into_safe_op(maindevice).await?;

        self_.request_into_op(maindevice).await
    }

    /// Transition all SubDevices in the group from PRE-OP to INIT.
    pub async fn into_init(
        self,
        maindevice: &MainDevice<'_>,
    ) -> Result<SubDeviceGroup<MAX_SUBDEVICES, MAX_PDI, R, Init, DC>, Error> {
        self.transition_to(maindevice, SubDeviceState::Init).await
    }
}

impl<const MAX_SUBDEVICES: usize, const MAX_PDI: usize, R: RawRwLock, DC>
    SubDeviceGroup<MAX_SUBDEVICES, MAX_PDI, R, SafeOp, DC>
{
    /// Transition all SubDevices in the group from SAFE-OP to OP.
    pub async fn into_op(
        self,
        maindevice: &MainDevice<'_>,
    ) -> Result<SubDeviceGroup<MAX_SUBDEVICES, MAX_PDI, R, Op, DC>, Error> {
        self.transition_to(maindevice, SubDeviceState::Op).await
    }

    /// Transition all SubDevices in the group from SAFE-OP to PRE-OP.
    pub async fn into_pre_op(
        self,
        maindevice: &MainDevice<'_>,
    ) -> Result<SubDeviceGroup<MAX_SUBDEVICES, MAX_PDI, R, PreOp, DC>, Error> {
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
    ) -> Result<SubDeviceGroup<MAX_SUBDEVICES, MAX_PDI, R, Op, DC>, Error> {
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

impl<const MAX_SUBDEVICES: usize, const MAX_PDI: usize, R: RawRwLock, DC>
    SubDeviceGroup<MAX_SUBDEVICES, MAX_PDI, R, Op, DC>
{
    /// Transition all SubDevices in the group from OP to SAFE-OP.
    pub async fn into_safe_op(
        self,
        maindevice: &MainDevice<'_>,
    ) -> Result<SubDeviceGroup<MAX_SUBDEVICES, MAX_PDI, R, SafeOp, DC>, Error> {
        self.transition_to(maindevice, SubDeviceState::SafeOp).await
    }
}

impl<const MAX_SUBDEVICES: usize, const MAX_PDI: usize, R: RawRwLock, S> Default
    for SubDeviceGroup<MAX_SUBDEVICES, MAX_PDI, R, S>
{
    fn default() -> Self {
        Self {
            id: GroupId(GROUP_ID.fetch_add(1, core::sync::atomic::Ordering::Relaxed)),
            pdi: RwLock::new(MySyncUnsafeCell::new([0u8; MAX_PDI])),
            read_pdi_len: Default::default(),
            pdi_len: Default::default(),
            inner: MySyncUnsafeCell::new(GroupInner::default()),
            dc_conf: NoDc,
            _state: PhantomData,
        }
    }
}

impl<const MAX_SUBDEVICES: usize, const MAX_PDI: usize, R: RawRwLock, S, DC>
    SubDeviceGroup<MAX_SUBDEVICES, MAX_PDI, R, S, DC>
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
                maindevice.timeouts.pdu(),
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
        .timeout(maindevice.timeouts.state_transition())
        .await
    }

    /// Transition to a new state without destroying the group.
    /// Found to be necessary for debugging where one needs to read SDOs or IDNs
    /// after a state transition fails
    pub async fn attempt_transition_to(
        &mut self,
        maindevice: &MainDevice<'_>,
        desired_state: SubDeviceState,
    ) -> Result<(), Error> {
        for subdevice in self.inner.get_mut().subdevices.iter_mut() {
            SubDeviceRef::new(maindevice, subdevice.configured_address(), subdevice)
                .request_subdevice_state_nowait(desired_state)
                .await?;
        }

        Ok(())
    }

    /// Transition to a new state.
    async fn transition_to<TO>(
        mut self,
        maindevice: &MainDevice<'_>,
        desired_state: SubDeviceState,
    ) -> Result<SubDeviceGroup<MAX_SUBDEVICES, MAX_PDI, R, TO, DC>, Error> {
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
impl<const MAX_SUBDEVICES: usize, const MAX_PDI: usize, R: RawRwLock, S, DC>
    SubDeviceGroup<MAX_SUBDEVICES, MAX_PDI, R, S, DC>
where
    S: HasPdi,
{
    /// Borrow an individual SubDevice.
    #[doc(alias = "slave")]
    pub fn subdevice<'maindevice, 'group>(
        &'group self,
        maindevice: &'maindevice MainDevice<'maindevice>,
        index: usize,
    ) -> Result<SubDeviceRef<'maindevice, SubDevicePdi<'group, MAX_PDI, R>>, Error> {
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

    /// Get an iterator over all SubDevices in this group.
    pub fn iter<'group, 'maindevice>(
        &'group self,
        maindevice: &'maindevice MainDevice<'maindevice>,
    ) -> impl Iterator<Item = SubDeviceRef<'group, SubDevicePdi<'group, MAX_PDI, R>>>
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
}
