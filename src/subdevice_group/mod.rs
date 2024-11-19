//! A group of SubDevices.
//!
//! SubDevices can be divided into multiple groups to allow multiple tasks to run concurrently,
//! potentially at different tick rates.

mod configurator;
mod group_id;
mod handle;
mod iterator;

use crate::{
    al_control::AlControl,
    command::Command,
    error::{DistributedClockError, Error, Item, PduError},
    fmt,
    pdi::PdiOffset,
    pdu_loop::{CreatedFrame, ReceivedPdu},
    subdevice::{
        configuration::PdoDirection, pdi::SubDevicePdi, IoRanges, SubDevice, SubDeviceRef,
    },
    timer_factory::IntoTimeout,
    DcSync, MainDevice, RegisterAddress, SubDeviceState,
};
use atomic_refcell::{AtomicRefCell, AtomicRefMut};
use core::{
    cell::UnsafeCell, marker::PhantomData, slice, sync::atomic::AtomicUsize, time::Duration,
};
use ethercrab_wire::{EtherCrabWireRead, EtherCrabWireSized};

pub use self::group_id::GroupId;
pub use self::handle::SubDeviceGroupHandle;
pub use self::iterator::GroupSubDeviceIterator;
pub use configurator::SubDeviceGroupRef;

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
    subdevices: heapless::Vec<AtomicRefCell<SubDevice>, MAX_SUBDEVICES>,
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
    pdi: UnsafeCell<[u8; MAX_PDI]>,
    /// The number of bytes at the beginning of the PDI reserved for SubDevice inputs.
    read_pdi_len: usize,
    /// The total length (I and O) of the PDI for this group.
    pdi_len: usize,
    inner: UnsafeCell<GroupInner<MAX_SUBDEVICES>>,
    dc_conf: DC,
    _state: PhantomData<S>,
}

impl<const MAX_SUBDEVICES: usize, const MAX_PDI: usize, DC>
    SubDeviceGroup<MAX_SUBDEVICES, MAX_PDI, PreOp, DC>
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
        for subdevice in inner.subdevices.iter_mut().map(AtomicRefCell::get_mut) {
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
        for subdevice in inner.subdevices.iter_mut().map(AtomicRefCell::get_mut) {
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
    ///
    /// Each SubDevice in the group is wrapped in an `AtomicRefCell`, meaning it may only have a
    /// single reference to it at any one time. Multiple different SubDevices can be borrowed
    /// simultaneously, but multiple references to the same SubDevice are not allowed.
    ///
    /// # Errors
    ///
    /// This method will return an error if the given index is out of range of the current group, or
    /// if the SubDevice at the given index is already borrowed.
    #[deny(clippy::panic)]
    #[doc(alias = "slave")]
    pub fn subdevice<'maindevice, 'group>(
        &'group self,
        maindevice: &'maindevice MainDevice<'maindevice>,
        index: usize,
    ) -> Result<SubDeviceRef<'maindevice, AtomicRefMut<'group, SubDevice>>, Error> {
        let subdevice = self
            .inner()
            .subdevices
            .get(index)
            .ok_or(Error::NotFound {
                item: Item::SubDevice,
                index: Some(index),
            })?
            .try_borrow_mut()
            .map_err(|_e| {
                fmt::error!("SubDevice index {} already borrowed", index);

                Error::Borrow
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
    ) -> Result<SubDeviceGroup<MAX_SUBDEVICES, MAX_PDI, Op, DC>, Error> {
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
    ) -> Result<SubDeviceGroup<MAX_SUBDEVICES, MAX_PDI, PreOpPdi, DC>, Error> {
        self.configure_fmmus(maindevice).await?;

        Ok(SubDeviceGroup {
            id: self.id,
            pdi: self.pdi,
            read_pdi_len: self.read_pdi_len,
            pdi_len: self.pdi_len,
            inner: UnsafeCell::new(self.inner.into_inner()),
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
        &'group mut self,
        maindevice: &'maindevice MainDevice<'maindevice>,
    ) -> GroupSubDeviceIterator<'group, 'maindevice, MAX_SUBDEVICES, MAX_PDI, PreOp, DC> {
        GroupSubDeviceIterator::new(maindevice, self)
    }
}

impl<const MAX_SUBDEVICES: usize, const MAX_PDI: usize, S, DC>
    SubDeviceGroup<MAX_SUBDEVICES, MAX_PDI, S, DC>
where
    S: IsPreOp,
{
    /// Configure Distributed Clock SYNC0 for all SubDevices in this group.
    ///
    /// # Errors
    ///
    /// This method will return with a
    /// [`Error::DistributedClock(DistributedClockError::NoReference)`](Error::DistributedClock)
    /// error if no DC reference SubDevice is present on the network.
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
            inner: UnsafeCell::new(self.inner.into_inner()),
            dc_conf: NoDc,
            _state: PhantomData::<PreOp>,
        };

        // Only configure DC for those devices that want and support it
        let dc_devices = GroupSubDeviceIterator::new(maindevice, &self_).filter(|subdevice| {
            subdevice.dc_support().any() && !matches!(subdevice.dc_sync(), DcSync::Disabled)
        });

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

            let device_time: u64 = subdevice
                .read(RegisterAddress::DcSystemTime)
                .ignore_wkc()
                .receive(maindevice)
                .await?;

            fmt::debug!("--> Device time {} ns", device_time);

            let sync0_period = sync0_period.as_nanos() as u64;

            let first_pulse_delay = start_delay.as_nanos() as u64;

            // Round first pulse time to a whole number of cycles
            let start_time = (device_time + first_pulse_delay) / sync0_period * sync0_period;

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
                subdevice
                    .write(RegisterAddress::DcSync1CycleTime)
                    .send(maindevice, sync1_period.as_nanos() as u64)
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
            inner: UnsafeCell::new(self_.inner.into_inner()),
            dc_conf: HasDc {
                sync0_period: sync0_period.as_nanos() as u64,
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

    /// Like [`into_op`](SubDeviceGroup::into_op), however does not wait for all SubDevices to enter OP
    /// state.
    ///
    /// This allows the application process data loop to be started, so as to e.g. not time out
    /// watchdogs, or provide valid data to prevent DC sync errors.
    ///
    /// If the SubDevice status is not mapped to the PDI, use [`all_op`](SubDeviceGroup::all_op) to
    /// check if the group has reached OP state.
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
    /// If the SubDevice status is not mapped to the PDI, use [`all_op`](SubDeviceGroup::all_op) to
    /// check if the group has reached OP state.
    pub async fn request_into_op(
        mut self,
        maindevice: &MainDevice<'_>,
    ) -> Result<SubDeviceGroup<MAX_SUBDEVICES, MAX_PDI, Op, DC>, Error> {
        for subdevice in self
            .inner
            .get_mut()
            .subdevices
            .iter_mut()
            .map(|subdevice| subdevice.get_mut())
        {
            SubDeviceRef::new(maindevice, subdevice.configured_address(), subdevice)
                .request_subdevice_state_nowait(SubDeviceState::Op)
                .await?;
        }

        Ok(SubDeviceGroup {
            id: self.id,
            pdi: self.pdi,
            read_pdi_len: self.read_pdi_len,
            pdi_len: self.pdi_len,
            inner: UnsafeCell::new(self.inner.into_inner()),
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

    /// Returns true if all SubDevices in the group are in OP state
    pub async fn all_op(&self, maindevice: &MainDevice<'_>) -> Result<bool, Error> {
        self.is_state(maindevice, SubDeviceState::Op).await
    }
}

unsafe impl<const MAX_SUBDEVICES: usize, const MAX_PDI: usize, S> Sync
    for SubDeviceGroup<MAX_SUBDEVICES, MAX_PDI, S>
{
}
unsafe impl<const MAX_SUBDEVICES: usize, const MAX_PDI: usize, S, DC> Send
    for SubDeviceGroup<MAX_SUBDEVICES, MAX_PDI, S, DC>
{
}

impl<const MAX_SUBDEVICES: usize, const MAX_PDI: usize, S> Default
    for SubDeviceGroup<MAX_SUBDEVICES, MAX_PDI, S>
{
    fn default() -> Self {
        Self {
            id: GroupId(GROUP_ID.fetch_add(1, core::sync::atomic::Ordering::Relaxed)),
            pdi: UnsafeCell::new([0u8; MAX_PDI]),
            read_pdi_len: Default::default(),
            pdi_len: Default::default(),
            inner: UnsafeCell::new(GroupInner::default()),
            dc_conf: NoDc,
            _state: PhantomData,
        }
    }
}

/// Returned when a SubDevice's input or output PDI segment is empty.
static EMPTY_PDI_SLICE: &[u8] = &[];

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

    #[allow(clippy::mut_from_ref)]
    fn pdi_mut(&self) -> &mut [u8] {
        let all_buf = unsafe { &mut *self.pdi.get() };

        &mut all_buf[0..self.pdi_len]
    }

    fn pdi(&self) -> &[u8] {
        let all_buf = unsafe { &*self.pdi.get() };

        &all_buf[0..self.pdi_len]
    }

    /// Check if all SubDevices in the group are the given desired state.
    async fn is_state(
        &self,
        maindevice: &MainDevice<'_>,
        desired_state: SubDeviceState,
    ) -> Result<bool, Error> {
        fmt::trace!("Check group state");

        let mut subdevices = self.inner().subdevices.iter().peekable();

        let mut frame_idx = 0;

        let mut total_checks = 0;

        // Send as many frames as required to check statuses of all subdevices
        loop {
            let mut frame = maindevice.pdu_loop.alloc_frame()?;

            let mut num_in_this_frame = 0;

            // Fill frame with status requests
            while let Some(sd) = subdevices.peek() {
                match frame.push_pdu(
                    Command::fprd(
                        sd.borrow().configured_address(),
                        RegisterAddress::AlStatus.into(),
                    )
                    .into(),
                    (),
                    Some(AlControl::PACKED_LEN as u16),
                ) {
                    Ok(_) => {
                        total_checks += 1;

                        // We peeked at the current item, so now we need to remove it from the
                        // iterator because it was successfully inserted into the frame.
                        let _ = subdevices.next();
                    }
                    // Frame is full, we'll do more next time round
                    Err(PduError::TooLong) => {
                        fmt::trace!(
                            "--> Pushed {} checks into frame {}",
                            num_in_this_frame,
                            frame_idx
                        );

                        break;
                    }
                    // Bail on a legitimate failure
                    Err(e) => return Err(e.into()),
                }

                num_in_this_frame += 1;

                // A status check datagram is 14 bytes, meaning we can fit at most just over 100
                // checks per normal EtherCAT frame. This leaves spare PDU indices available for
                // other purposes, however if the user is using jumbo frames or something, we should
                // always leave some indices free for e.g. other threads.
                if num_in_this_frame > 128 {
                    break;
                }
            }

            // Nothing to send, we've checked all SDs
            if num_in_this_frame == 0 {
                fmt::trace!("--> No more state checks, pushed {}", total_checks);

                break;
            }

            let frame = frame.mark_sendable(
                &maindevice.pdu_loop,
                maindevice.timeouts.pdu,
                maindevice.config.retry_behaviour.retry_count(),
            );

            maindevice.pdu_loop.wake_sender();

            let received = frame.await?;

            for pdu in received.into_iter() {
                let pdu = pdu?;

                let result = AlControl::unpack_from_slice(&pdu)?;

                if result.state != desired_state {
                    return Ok(false);
                }
            }

            frame_idx += 1;
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
        for subdevice in self
            .inner
            .get_mut()
            .subdevices
            .iter_mut()
            .map(AtomicRefCell::get_mut)
        {
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
            inner: UnsafeCell::new(self.inner.into_inner()),
            dc_conf: self.dc_conf,
            _state: PhantomData,
        })
    }
}

// Methods for any state where a PDI has been configured.
impl<const MAX_SUBDEVICES: usize, const MAX_PDI: usize, S, DC>
    SubDeviceGroup<MAX_SUBDEVICES, MAX_PDI, S, DC>
where
    S: HasPdi,
{
    /// Borrow an individual SubDevice.
    ///
    /// Each SubDevice in the group is wrapped in an `AtomicRefCell`, meaning it may only have a
    /// single reference to it at any one time. Multiple different SubDevices can be borrowed
    /// simultaneously, but multiple references to the same SubDevice are not allowed.
    #[doc(alias = "slave")]
    pub fn subdevice<'maindevice, 'group>(
        &'group self,
        maindevice: &'maindevice MainDevice<'maindevice>,
        index: usize,
    ) -> Result<SubDeviceRef<'maindevice, SubDevicePdi<'group>>, Error> {
        let subdevice = self
            .inner()
            .subdevices
            .get(index)
            .ok_or(Error::NotFound {
                item: Item::SubDevice,
                index: Some(index),
            })?
            .try_borrow_mut()
            .map_err(|_e| {
                fmt::error!("SubDevice index {} already borrowed", index);

                Error::Borrow
            })?;

        let IoRanges {
            input: input_range,
            output: output_range,
        } = subdevice.io_segments();

        // SAFETY: Multiple references are ok as long as I and O ranges do not overlap.
        let i_data = self.pdi();
        let o_data = self.pdi_mut();

        fmt::trace!(
            "Get SubDevice {:#06x} IO ranges I: {}, O: {}",
            subdevice.configured_address(),
            input_range,
            output_range
        );

        fmt::trace!(
            "--> Group PDI: {:?} ({} byte subset of {} max)",
            i_data,
            self.pdi_len,
            MAX_PDI
        );

        // NOTE: Using panicking `[]` indexing as the indices and arrays should all be correct by
        // this point. If something isn't right, that's a bug.
        let inputs = if input_range.is_empty() {
            EMPTY_PDI_SLICE
        } else {
            i_data
                .get(input_range.bytes.clone())
                .ok_or(Error::Internal)?
        };

        let outputs = if output_range.is_empty() {
            // SAFETY: Slice is empty so can never be mutated
            unsafe { slice::from_raw_parts_mut(EMPTY_PDI_SLICE.as_ptr().cast_mut(), 0) }
        } else {
            o_data
                .get_mut(output_range.bytes.clone())
                .ok_or(Error::Internal)?
        };

        Ok(SubDeviceRef::new(
            maindevice,
            subdevice.configured_address(),
            // SAFETY: A given SubDevice contained in a `SubDevicePdi` MUST only be borrowed once
            // (currently enforced by `AtomicRefCell`). If it is borrowed more than once, immutable
            // APIs in `SubDeviceRef<SubDevicePdi>` will be unsound.
            SubDevicePdi::new(subdevice, inputs, outputs),
        ))
    }

    /// Get an iterator over all SubDevices in this group.
    pub fn iter<'group, 'maindevice>(
        &'group mut self,
        maindevice: &'maindevice MainDevice<'maindevice>,
    ) -> GroupSubDeviceIterator<'group, 'maindevice, MAX_SUBDEVICES, MAX_PDI, S, DC> {
        GroupSubDeviceIterator::new(maindevice, self)
    }

    /// Drive the SubDevice group's inputs and outputs.
    ///
    /// A `SubDeviceGroup` will not process any inputs or outputs unless this method is called
    /// periodically. It will send an `LRW` to update SubDevice outputs and read SubDevice inputs.
    ///
    /// This method returns the working counter on success.
    ///
    /// # Errors
    ///
    /// This method will return with an error if the PDU could not be sent over the network, or the
    /// response times out.
    pub async fn tx_rx<'sto>(&self, maindevice: &'sto MainDevice<'sto>) -> Result<u16, Error> {
        fmt::trace!(
            "Group TX/RX, start address {:#010x}, data len {}, of which read bytes: {}",
            self.inner().pdi_start.start_address,
            self.pdi().len(),
            self.read_pdi_len
        );

        let mut remaining = self.pdi();
        let mut total_bytes_sent = 0;
        let mut lrw_wkc_sum = 0;

        while !remaining.is_empty() {
            let mut frame = maindevice.pdu_loop.alloc_frame()?;

            let start_addr = self.inner().pdi_start.start_address + total_bytes_sent as u32;

            let Some((bytes_in_this_chunk, pdu_handle)) =
                frame.push_pdu_slice_rest(Command::lrw(start_addr).into(), remaining)?
            else {
                continue;
            };

            remaining = &remaining[bytes_in_this_chunk..];

            let frame = frame.mark_sendable(
                &maindevice.pdu_loop,
                maindevice.timeouts.pdu,
                maindevice.config.retry_behaviour.retry_count(),
            );

            maindevice.pdu_loop.wake_sender();

            let received = frame.await?;

            let wkc = self.process_received_pdi_chunk(
                total_bytes_sent,
                bytes_in_this_chunk,
                &received.pdu(pdu_handle)?,
            )?;

            total_bytes_sent += bytes_in_this_chunk;
            lrw_wkc_sum += wkc;
        }

        Ok(lrw_wkc_sum)
    }

    /// Drive the SubDevice group's inputs and outputs and synchronise EtherCAT system time with
    /// `FRMW`.
    ///
    /// A `SubDeviceGroup` will not process any inputs or outputs unless this method is called
    /// periodically. It will send an `LRW` to update SubDevice outputs and read SubDevice inputs.
    ///
    /// This method returns the working counter and the current EtherCAT system time in nanoseconds
    /// on success. If the PDI must be sent in multiple chunks, the returned working counter is the
    /// sum of all returned working counter values.
    ///
    /// # Errors
    ///
    /// This method will return with an error if the PDU could not be sent over the network, or the
    /// response times out.
    pub async fn tx_rx_sync_system_time<'sto>(
        &self,
        maindevice: &'sto MainDevice<'sto>,
    ) -> Result<(u16, Option<u64>), Error> {
        fmt::trace!(
            "Group TX/RX with DC sync, start address {:#010x}, data len {}, of which read bytes: {}",
            self.inner().pdi_start.start_address,
            self.pdi().len(),
            self.read_pdi_len
        );

        if let Some(dc_ref) = maindevice.dc_ref_address() {
            let mut remaining = self.pdi();
            let mut total_bytes_sent = 0;
            let mut time = 0;
            let mut lrw_wkc_sum = 0;
            let mut time_read = false;

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

                let start_addr = self.inner().pdi_start.start_address + total_bytes_sent as u32;

                let Some((bytes_in_this_chunk, pdu_handle)) =
                    frame.push_pdu_slice_rest(Command::lrw(start_addr).into(), remaining)?
                else {
                    continue;
                };

                fmt::trace!("Wrote {} byte chunk", bytes_in_this_chunk);

                remaining = &remaining[bytes_in_this_chunk..];

                let frame = frame.mark_sendable(
                    &maindevice.pdu_loop,
                    maindevice.timeouts.pdu,
                    maindevice.config.retry_behaviour.retry_count(),
                );

                maindevice.pdu_loop.wake_sender();

                let received = frame.await?;

                if let Some(dc_handle) = dc_handle {
                    time = received
                        .pdu(dc_handle)
                        .and_then(|rx| u64::unpack_from_slice(&rx).map_err(Error::from))?;

                    time_read = true;
                }

                let wkc = self.process_received_pdi_chunk(
                    total_bytes_sent,
                    bytes_in_this_chunk,
                    &received.pdu(pdu_handle)?,
                )?;

                total_bytes_sent += bytes_in_this_chunk;
                lrw_wkc_sum += wkc;

                // NOTE: Not using a while loop as we want to always send the DC sync PDU even if
                // the PDI is empty.
                if remaining.is_empty() {
                    break Ok((lrw_wkc_sum, Some(time)));
                }
            }
        } else {
            self.tx_rx(maindevice).await.map(|wkc| (wkc, None))
        }
    }

    fn process_received_pdi_chunk(
        &self,
        total_bytes_sent: usize,
        bytes_in_this_chunk: usize,
        data: &ReceivedPdu<'_>,
    ) -> Result<u16, Error> {
        let wkc = data.working_counter;

        // If we've read the inputs chunk, write it back into the PDI (PDI is organised as
        // IIIIOOOO)
        if bytes_in_this_chunk > 0 && total_bytes_sent < self.read_pdi_len {
            let inputs_range =
                total_bytes_sent..(total_bytes_sent + self.read_pdi_len.min(bytes_in_this_chunk));

            self.pdi_mut()
                .get_mut(inputs_range.clone())
                .ok_or(Error::Internal)?
                .copy_from_slice(data.get(0..inputs_range.len()).ok_or(Error::Internal)?);
        }

        Ok(wkc)
    }
}

// Methods for when the group has a PDI AND has Distributed Clocks configured
impl<const MAX_SUBDEVICES: usize, const MAX_PDI: usize, S>
    SubDeviceGroup<MAX_SUBDEVICES, MAX_PDI, S, HasDc>
where
    S: HasPdi,
{
    /// Drive the SubDevice group's inputs and outputs, synchronise EtherCAT system time with `FRMW`,
    /// and return cycle timing information.
    ///
    /// A `SubDeviceGroup` will not process any inputs or outputs unless this method is called
    /// periodically. It will send an `LRW` to update SubDevice outputs and read SubDevice inputs.
    ///
    /// This method returns the working counter and a [`CycleInfo`], containing values that can be
    /// used to synchronise the MainDevice to the network SYNC0 event.
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
    /// #     subdevice_group::{CycleInfo, DcConfiguration},
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
    /// for mut subdevice in group.iter(&maindevice) {
    ///     subdevice.set_dc_sync(DcSync::Sync0);
    /// }
    ///
    /// let group = group
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
    ///     let (
    ///         _wkc,
    ///         CycleInfo {
    ///             next_cycle_wait, ..
    ///         },
    ///     ) = group.tx_rx_dc(&maindevice).await.expect("TX/RX");
    ///
    ///     if group.all_op(&maindevice).await? {
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
    ///     let (
    ///         _wkc,
    ///         CycleInfo {
    ///             next_cycle_wait, ..
    ///         },
    ///     ) = group.tx_rx_dc(&maindevice).await.expect("TX/RX");
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
    ) -> Result<(u16, CycleInfo), Error> {
        fmt::trace!(
            "Group TX/RX with DC sync, start address {:#010x}, data len {}, of which read bytes: {}",
            self.inner().pdi_start.start_address,
            self.pdi().len(),
            self.read_pdi_len
        );

        let mut remaining = self.pdi();
        let mut total_bytes_sent = 0;
        let mut time = 0;
        let mut lrw_wkc_sum = 0;
        let mut time_read = false;

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

            let start_addr = self.inner().pdi_start.start_address + total_bytes_sent as u32;

            let Some((bytes_in_this_chunk, pdu_handle)) =
                frame.push_pdu_slice_rest(Command::lrw(start_addr).into(), remaining)?
            else {
                continue;
            };

            remaining = &remaining[bytes_in_this_chunk..];

            let frame = frame.mark_sendable(
                &maindevice.pdu_loop,
                maindevice.timeouts.pdu,
                maindevice.config.retry_behaviour.retry_count(),
            );

            maindevice.pdu_loop.wake_sender();

            let received = frame.await?;

            if let Some(dc_handle) = dc_handle {
                time = received
                    .pdu(dc_handle)
                    .and_then(|rx| u64::unpack_from_slice(&rx).map_err(Error::from))?;

                time_read = true;
            }

            let wkc = self.process_received_pdi_chunk(
                total_bytes_sent,
                bytes_in_this_chunk,
                &received.pdu(pdu_handle)?,
            )?;

            total_bytes_sent += bytes_in_this_chunk;
            lrw_wkc_sum += wkc;

            // NOTE: Not using a while loop as we want to always send the DC sync PDU even if the
            // PDI is empty.
            if remaining.is_empty() {
                break;
            }
        }

        // Nanoseconds from the start of the cycle. This works because the first SYNC0 pulse
        // time is rounded to a whole number of `sync0_period`-length cycles.
        let cycle_start_offset = time % self.dc_conf.sync0_period;

        let time_to_next_iter =
            (self.dc_conf.sync0_period - cycle_start_offset) + self.dc_conf.sync0_shift;

        Ok((
            lrw_wkc_sum,
            CycleInfo {
                dc_system_time: time,
                cycle_start_offset: Duration::from_nanos(cycle_start_offset),
                next_cycle_wait: Duration::from_nanos(time_to_next_iter),
            },
        ))
    }
}
