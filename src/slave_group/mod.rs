//! A group of slave devices.
//!
//! Slaves can be divided into multiple groups to allow multiple tasks to run concurrently,
//! potentially at different tick rates.

mod configurator;
mod group_id;
mod handle;
mod iterator;

use crate::{
    command::Command,
    error::{DistributedClockError, Error, Item, PduError},
    fmt,
    pdi::PdiOffset,
    slave::{configuration::PdoDirection, pdi::SlavePdi, IoRanges, Slave, SlaveRef},
    timer_factory::IntoTimeout,
    Client, DcSync, RegisterAddress, SlaveState,
};
use atomic_refcell::{AtomicRefCell, AtomicRefMut};
use core::{
    cell::UnsafeCell, marker::PhantomData, slice, sync::atomic::AtomicUsize, time::Duration,
};
use ethercrab_wire::EtherCrabWireRead;

pub use self::group_id::GroupId;
pub use self::handle::SlaveGroupHandle;
pub use self::iterator::GroupSlaveIterator;
pub use configurator::SlaveGroupRef;

static GROUP_ID: AtomicUsize = AtomicUsize::new(0);

/// A typestate for [`SlaveGroup`] representing a group that is shut down.
///
/// This corresponds to the EtherCAT states INIT.
#[derive(Copy, Clone, Debug)]
pub struct Init;

/// A typestate for [`SlaveGroup`] representing a group that is undergoing initialisation.
///
/// This corresponds to the EtherCAT states INIT and PRE-OP.
#[derive(Copy, Clone, Debug)]
pub struct PreOp;

/// The same as [`PreOp`] but with access to PDI methods. All slave configuration should be complete
/// at this point.
#[derive(Copy, Clone, Debug)]
pub struct PreOpPdi;

/// A typestate for [`SlaveGroup`] representing a group that is in SAFE-OP.
#[derive(Copy, Clone, Debug)]
pub struct SafeOp;

/// A typestate for [`SlaveGroup`] representing a group that is in OP.
#[derive(Copy, Clone, Debug)]
pub struct Op;

/// A typestate for [`SlaveGroup`]s that do not have a Distributed Clock configuration
#[derive(Copy, Clone, Debug)]
pub struct NoDc;

/// A typestate for [`SlaveGroup`]s that have a configured Distributed Clock.
///
/// This typestate can be entered by calling [`SlaveGroup::configure_dc_sync`].
#[derive(Copy, Clone, Debug)]
pub struct HasDc {
    sync0_period: u64,
    sync0_shift: u64,
    /// Configured address of the DC reference SubDevice.
    reference: u16,
}

/// Marker trait for `SlaveGroup` typestates where all SubDevices have a PDI.
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
struct GroupInner<const MAX_SLAVES: usize> {
    slaves: heapless::Vec<AtomicRefCell<Slave>, MAX_SLAVES>,
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
    /// [`sync0_shift`](DcConfiguration::sync0_shift) passed into [`SlaveGroup::configure_dc_sync`]
    /// and is meant to be used to accurately synchronise the MainDevice process data cycle with the
    /// DC system time.
    pub next_cycle_wait: Duration,

    /// The difference between the SYNC0 pulse and when the current cycle's data was received by the
    /// DC reference SubDevice.
    pub cycle_start_offset: Duration,
}

/// A group of one or more EtherCAT slaves.
///
/// Groups are created during EtherCrab initialisation, and are the only way to access individual
/// slave PDI sections.
pub struct SlaveGroup<const MAX_SLAVES: usize, const MAX_PDI: usize, S = PreOp, DC = NoDc> {
    id: GroupId,
    pdi: UnsafeCell<[u8; MAX_PDI]>,
    /// The number of bytes at the beginning of the PDI reserved for slave inputs.
    read_pdi_len: usize,
    /// The total length (I and O) of the PDI for this group.
    pdi_len: usize,
    inner: UnsafeCell<GroupInner<MAX_SLAVES>>,
    dc_conf: DC,
    _state: PhantomData<S>,
}

impl<const MAX_SLAVES: usize, const MAX_PDI: usize, DC> SlaveGroup<MAX_SLAVES, MAX_PDI, PreOp, DC> {
    /// Configure read/write FMMUs and PDI for this group.
    async fn configure_fmmus(&mut self, client: &Client<'_>) -> Result<(), Error> {
        let inner = self.inner.get_mut();

        let mut pdi_position = inner.pdi_start;

        fmt::debug!(
            "Going to configure group with {} slave(s), starting PDI offset {:#010x}",
            inner.slaves.len(),
            inner.pdi_start.start_address
        );

        // Configure master read PDI mappings in the first section of the PDI
        for slave in inner.slaves.iter_mut().map(|slave| slave.get_mut()) {
            // We're in PRE-OP at this point
            pdi_position = SlaveRef::new(client, slave.configured_address, slave)
                .configure_fmmus(
                    pdi_position,
                    inner.pdi_start.start_address,
                    PdoDirection::MasterRead,
                )
                .await?;
        }

        self.read_pdi_len = (pdi_position.start_address - inner.pdi_start.start_address) as usize;

        fmt::debug!("Slave mailboxes configured and init hooks called");

        // We configured all read PDI mappings as a contiguous block in the previous loop. Now we'll
        // configure the write mappings in a separate loop. This means we have IIIIOOOO instead of
        // IOIOIO.
        for slave in inner.slaves.iter_mut().map(|slave| slave.get_mut()) {
            let addr = slave.configured_address;

            let mut slave_config = SlaveRef::new(client, addr, slave);

            // Still in PRE-OP
            pdi_position = slave_config
                .configure_fmmus(
                    pdi_position,
                    inner.pdi_start.start_address,
                    PdoDirection::MasterWrite,
                )
                .await?;
        }

        fmt::debug!("Slave FMMUs configured for group. Able to move to SAFE-OP");

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

    /// Borrow an individual slave device.
    ///
    /// Each slave device in the group is wrapped in an `AtomicRefCell`, meaning it may only have a
    /// single reference to it at any one time. Multiple different slaves can be borrowed
    /// simultaneously, but multiple references to the same slave are not allowed.
    ///
    /// # Panics
    ///
    /// Borrowing a slave across a [`SlaveGroup::iter`](crate::SlaveGroup::iter) call will cause the
    /// returned iterator to panic as it tries to borrow the slave a second time.
    pub fn slave<'client, 'group>(
        &'group self,
        client: &'client Client<'client>,
        index: usize,
    ) -> Result<SlaveRef<'client, AtomicRefMut<'group, Slave>>, Error> {
        let slave = self
            .inner()
            .slaves
            .get(index)
            .ok_or(Error::NotFound {
                item: Item::Slave,
                index: Some(index),
            })?
            .try_borrow_mut()
            .map_err(|_e| {
                fmt::error!("Slave index {} already borrowed", index);

                Error::Borrow
            })?;

        Ok(SlaveRef::new(client, slave.configured_address, slave))
    }

    /// Transition the group from PRE-OP -> SAFE-OP -> OP.
    ///
    /// To transition individually from PRE-OP to SAFE-OP, then SAFE-OP to OP, see
    /// [`SlaveGroup::into_safe_op`].
    pub async fn into_op(
        self,
        client: &Client<'_>,
    ) -> Result<SlaveGroup<MAX_SLAVES, MAX_PDI, Op, DC>, Error> {
        let self_ = self.into_safe_op(client).await?;

        self_.into_op(client).await
    }

    /// Configure FMMUs, but leave the group in [`PreOp`] state.
    ///
    /// This method is used to obtain access to the group's PDI and related functionality. All SDO
    /// and other configuration should be complete at this point otherwise issues with cyclic data
    /// may occur (e.g. incorrect lengths, misplaced fields, etc).
    pub async fn into_pre_op_pdi(
        mut self,
        client: &Client<'_>,
    ) -> Result<SlaveGroup<MAX_SLAVES, MAX_PDI, PreOpPdi, DC>, Error> {
        self.configure_fmmus(client).await?;

        Ok(SlaveGroup {
            id: self.id,
            pdi: self.pdi,
            read_pdi_len: self.read_pdi_len,
            pdi_len: self.pdi_len,
            inner: UnsafeCell::new(self.inner.into_inner()),
            dc_conf: self.dc_conf,
            _state: PhantomData,
        })
    }

    /// Transition the slave group from PRE-OP to SAFE-OP.
    pub async fn into_safe_op(
        self,
        client: &Client<'_>,
    ) -> Result<SlaveGroup<MAX_SLAVES, MAX_PDI, SafeOp, DC>, Error> {
        let self_ = self.into_pre_op_pdi(client).await?;

        // We're done configuring FMMUs, etc, now we can request all slaves in this group go into
        // SAFE-OP
        self_.transition_to(client, SlaveState::SafeOp).await
    }

    /// Transition all slave devices in the group from PRE-OP to INIT.
    pub async fn into_init(
        self,
        client: &Client<'_>,
    ) -> Result<SlaveGroup<MAX_SLAVES, MAX_PDI, Init, DC>, Error> {
        self.transition_to(client, SlaveState::Init).await
    }

    /// Get an iterator over all slaves in this group.
    pub fn iter<'group, 'client>(
        &'group mut self,
        client: &'client Client<'client>,
    ) -> GroupSlaveIterator<'group, 'client, MAX_SLAVES, MAX_PDI, PreOp, DC> {
        GroupSlaveIterator::new(client, self)
    }
}

impl<const MAX_SLAVES: usize, const MAX_PDI: usize, S, DC> SlaveGroup<MAX_SLAVES, MAX_PDI, S, DC>
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
        client: &Client<'_>,
        dc_conf: DcConfiguration,
    ) -> Result<SlaveGroup<MAX_SLAVES, MAX_PDI, PreOpPdi, HasDc>, Error> {
        fmt::debug!("Configuring distributed clocks for group");

        let Some(reference) = client.dc_ref_address() else {
            fmt::error!("No DC reference clock SubDevice present, unable to configure DC");

            return Err(DistributedClockError::NoReference.into());
        };

        let DcConfiguration {
            start_delay,
            sync0_period,
            sync0_shift,
        } = dc_conf;

        // Coerce generics into concrete `PreOp` type as we don't need the PDI to configure the DC.
        let self_ = SlaveGroup {
            id: self.id,
            pdi: self.pdi,
            read_pdi_len: self.read_pdi_len,
            pdi_len: self.pdi_len,
            inner: UnsafeCell::new(self.inner.into_inner()),
            dc_conf: NoDc,
            _state: PhantomData::<PreOp>,
        };

        // Only configure DC for those devices that want and support it
        let dc_devices = GroupSlaveIterator::new(client, &self_).filter(|slave| {
            slave.dc_support().any() && !matches!(slave.dc_sync(), DcSync::Disabled)
        });

        for slave in dc_devices {
            fmt::debug!(
                "--> Configuring SubDevice {:#06x} {} DC mode {}",
                slave.configured_address(),
                slave.name(),
                slave.dc_sync()
            );

            // Disable cyclic op, ignore WKC
            slave
                .write(RegisterAddress::DcSyncActive)
                .ignore_wkc()
                .send(client, 0u8)
                .await?;

            // Write access to EtherCAT
            slave
                .write(RegisterAddress::DcCyclicUnitControl)
                .send(client, 0u8)
                .await?;

            let device_time: u64 = slave
                .read(RegisterAddress::DcSystemTime)
                .ignore_wkc()
                .receive(client)
                .await?;

            fmt::debug!("--> Device time {} ns", device_time);

            let sync0_period = sync0_period.as_nanos() as u64;

            let first_pulse_delay = start_delay.as_nanos() as u64;

            // Round first pulse time to a whole number of cycles
            let start_time = (device_time + first_pulse_delay) / sync0_period * sync0_period;

            fmt::debug!("--> Computed DC sync start time: {}", start_time);

            slave
                .write(RegisterAddress::DcSyncStartTime)
                .send(client, start_time)
                .await?;

            // Cycle time in nanoseconds
            slave
                .write(RegisterAddress::DcSync0CycleTime)
                .send(client, sync0_period)
                .await?;

            let flags = if let DcSync::Sync01 { sync1_period } = slave.dc_sync() {
                slave
                    .write(RegisterAddress::DcSync1CycleTime)
                    .send(client, sync1_period.as_nanos() as u64)
                    .await?;

                SYNC1_ACTIVATE | SYNC0_ACTIVATE | CYCLIC_OP_ENABLE
            } else {
                SYNC0_ACTIVATE | CYCLIC_OP_ENABLE
            };

            slave
                .write(RegisterAddress::DcSyncActive)
                .send(client, flags)
                .await?;
        }

        Ok(SlaveGroup {
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

impl<const MAX_SLAVES: usize, const MAX_PDI: usize, DC>
    SlaveGroup<MAX_SLAVES, MAX_PDI, PreOpPdi, DC>
{
    /// Transition the slave group from PRE-OP to SAFE-OP.
    pub async fn into_safe_op(
        self,
        client: &Client<'_>,
    ) -> Result<SlaveGroup<MAX_SLAVES, MAX_PDI, SafeOp, DC>, Error> {
        self.transition_to(client, SlaveState::SafeOp).await
    }

    /// Transition all slave devices in the group from PRE-OP to SAFE-OP, then to OP.
    ///
    /// This is a convenience method that calls [`into_safe_op`](SlaveGroup::into_safe_op) then
    /// [`into_op`](SlaveGroup::into_op).
    pub async fn into_op(
        self,
        client: &Client<'_>,
    ) -> Result<SlaveGroup<MAX_SLAVES, MAX_PDI, Op, DC>, Error> {
        let self_ = self.into_safe_op(client).await?;

        self_.transition_to(client, SlaveState::Op).await
    }

    /// Like [`into_op`](SlaveGroup::into_op), however does not wait for all SubDevices to enter OP
    /// state.
    ///
    /// This allows the application process data loop to be started, so as to e.g. not time out
    /// watchdogs, or provide valid data to prevent DC sync errors.
    ///
    /// If the SubDevice status is not mapped to the PDI, use [`all_op`](SlaveGroup::all_op) to
    /// check if the group has reached OP state.
    pub async fn request_into_op(
        self,
        client: &Client<'_>,
    ) -> Result<SlaveGroup<MAX_SLAVES, MAX_PDI, Op, DC>, Error> {
        let self_ = self.into_safe_op(client).await?;

        self_.request_into_op(client).await
    }

    /// Transition all slave devices in the group from PRE-OP to INIT.
    pub async fn into_init(
        self,
        client: &Client<'_>,
    ) -> Result<SlaveGroup<MAX_SLAVES, MAX_PDI, Init, DC>, Error> {
        self.transition_to(client, SlaveState::Init).await
    }
}

impl<const MAX_SLAVES: usize, const MAX_PDI: usize, DC>
    SlaveGroup<MAX_SLAVES, MAX_PDI, SafeOp, DC>
{
    /// Transition all slave devices in the group from SAFE-OP to OP.
    pub async fn into_op(
        self,
        client: &Client<'_>,
    ) -> Result<SlaveGroup<MAX_SLAVES, MAX_PDI, Op, DC>, Error> {
        self.transition_to(client, SlaveState::Op).await
    }

    /// Transition all slave devices in the group from SAFE-OP to PRE-OP.
    pub async fn into_pre_op(
        self,
        client: &Client<'_>,
    ) -> Result<SlaveGroup<MAX_SLAVES, MAX_PDI, PreOp, DC>, Error> {
        self.transition_to(client, SlaveState::PreOp).await
    }

    /// Like [`into_op`](SlaveGroup::into_op), however does not wait for all SubDevices to enter OP
    /// state.
    ///
    /// This allows the application process data loop to be started, so as to e.g. not time out
    /// watchdogs, or provide valid data to prevent DC sync errors.
    ///
    /// If the SubDevice status is not mapped to the PDI, use [`all_op`](SlaveGroup::all_op) to
    /// check if the group has reached OP state.
    pub async fn request_into_op(
        mut self,
        client: &Client<'_>,
    ) -> Result<SlaveGroup<MAX_SLAVES, MAX_PDI, Op, DC>, Error> {
        for slave in self
            .inner
            .get_mut()
            .slaves
            .iter_mut()
            .map(|slave| slave.get_mut())
        {
            SlaveRef::new(client, slave.configured_address, slave)
                .request_slave_state_nowait(SlaveState::Op)
                .await?;
        }

        Ok(SlaveGroup {
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

impl<const MAX_SLAVES: usize, const MAX_PDI: usize, DC> SlaveGroup<MAX_SLAVES, MAX_PDI, Op, DC> {
    /// Transition all slave devices in the group from OP to SAFE-OP.
    pub async fn into_safe_op(
        self,
        client: &Client<'_>,
    ) -> Result<SlaveGroup<MAX_SLAVES, MAX_PDI, SafeOp, DC>, Error> {
        self.transition_to(client, SlaveState::SafeOp).await
    }

    /// Returns true if all SubDevices in the group are in OP state
    pub async fn all_op(&self, client: &Client<'_>) -> Result<bool, Error> {
        self.is_state(client, SlaveState::Op).await
    }
}

unsafe impl<const MAX_SLAVES: usize, const MAX_PDI: usize, S> Sync
    for SlaveGroup<MAX_SLAVES, MAX_PDI, S>
{
}
unsafe impl<const MAX_SLAVES: usize, const MAX_PDI: usize, S, DC> Send
    for SlaveGroup<MAX_SLAVES, MAX_PDI, S, DC>
{
}

impl<const MAX_SLAVES: usize, const MAX_PDI: usize, S> Default
    for SlaveGroup<MAX_SLAVES, MAX_PDI, S>
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

/// Returned when a slave device's input or output PDI segment is empty.
static EMPTY_PDI_SLICE: &[u8] = &[];

impl<const MAX_SLAVES: usize, const MAX_PDI: usize, S, DC> SlaveGroup<MAX_SLAVES, MAX_PDI, S, DC> {
    fn inner(&self) -> &GroupInner<MAX_SLAVES> {
        unsafe { &*self.inner.get() }
    }

    /// Get the number of slave devices in this group.
    pub fn len(&self) -> usize {
        self.inner().slaves.len()
    }

    /// Check whether this slave group is empty or not.
    pub fn is_empty(&self) -> bool {
        self.inner().slaves.is_empty()
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
        client: &Client<'_>,
        desired_state: SlaveState,
    ) -> Result<bool, Error> {
        for slave in self.inner().slaves.iter().map(|slave| slave.borrow()) {
            let s = SlaveRef::new(client, slave.configured_address, slave);

            // TODO: Add a way to queue up a bunch of PDUs and send all at once
            let slave_state = s.state().await.map_err(|e| {
                fmt::error!(
                    "Failed to transition SubDevice {:#06x}: {}",
                    s.configured_address(),
                    e
                );

                e
            })?;

            if slave_state != desired_state {
                return Ok(false);
            }
        }

        Ok(true)
    }

    /// Wait for all slaves in this group to transition to the given state.
    async fn wait_for_state(
        &self,
        client: &Client<'_>,
        desired_state: SlaveState,
    ) -> Result<(), Error> {
        async {
            loop {
                if self.is_state(client, desired_state).await? {
                    break Ok(());
                }

                client.timeouts.loop_tick().await;
            }
        }
        .timeout(client.timeouts.state_transition)
        .await
    }

    /// Transition to a new state.
    async fn transition_to<TO>(
        mut self,
        client: &Client<'_>,
        desired_state: SlaveState,
    ) -> Result<SlaveGroup<MAX_SLAVES, MAX_PDI, TO, DC>, Error> {
        // We're done configuring FMMUs, etc, now we can request all slaves in this group go into
        // SAFE-OP
        for slave in self
            .inner
            .get_mut()
            .slaves
            .iter_mut()
            .map(|slave| slave.get_mut())
        {
            SlaveRef::new(client, slave.configured_address, slave)
                .request_slave_state_nowait(desired_state)
                .await?;
        }

        fmt::debug!("Waiting for group state {}", desired_state);

        self.wait_for_state(client, desired_state).await?;

        fmt::debug!("--> Group reached state {}", desired_state);

        Ok(SlaveGroup {
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
impl<const MAX_SLAVES: usize, const MAX_PDI: usize, S, DC> SlaveGroup<MAX_SLAVES, MAX_PDI, S, DC>
where
    S: HasPdi,
{
    /// Borrow an individual slave device.
    ///
    /// Each slave device in the group is wrapped in an `AtomicRefCell`, meaning it may only have a
    /// single reference to it at any one time. Multiple different slaves can be borrowed
    /// simultaneously, but multiple references to the same slave are not allowed.
    pub fn slave<'client, 'group>(
        &'group self,
        client: &'client Client<'client>,
        index: usize,
    ) -> Result<SlaveRef<'client, SlavePdi<'group>>, Error> {
        let slave = self
            .inner()
            .slaves
            .get(index)
            .ok_or(Error::NotFound {
                item: Item::Slave,
                index: Some(index),
            })?
            .try_borrow_mut()
            .map_err(|_e| {
                fmt::error!("Slave index {} already borrowed", index);

                Error::Borrow
            })?;

        let IoRanges {
            input: input_range,
            output: output_range,
        } = slave.io_segments();

        // SAFETY: Multiple references are ok as long as I and O ranges do not overlap.
        let i_data = self.pdi();
        let o_data = self.pdi_mut();

        fmt::trace!(
            "Get slave {:#06x} IO ranges I: {}, O: {}",
            slave.configured_address,
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
        let inputs = if !input_range.is_empty() {
            &i_data[input_range.bytes.clone()]
        } else {
            EMPTY_PDI_SLICE
        };

        let outputs = if !output_range.is_empty() {
            &mut o_data[output_range.bytes.clone()]
        } else {
            // SAFETY: Slice is empty so can never be mutated
            unsafe { slice::from_raw_parts_mut(EMPTY_PDI_SLICE.as_ptr() as *mut _, 0) }
        };

        Ok(SlaveRef::new(
            client,
            slave.configured_address,
            // SAFETY: A given slave contained in a `SlavePdi` MUST only be borrowed once (currently
            // enforced by `AtomicRefCell`). If it is borrowed more than once, immutable APIs in
            // `SlaveRef<SlavePdi>` will be unsound.
            SlavePdi::new(slave, inputs, outputs),
        ))
    }

    /// Get an iterator over all slaves in this group.
    pub fn iter<'group, 'client>(
        &'group mut self,
        client: &'client Client<'client>,
    ) -> GroupSlaveIterator<'group, 'client, MAX_SLAVES, MAX_PDI, S, DC> {
        GroupSlaveIterator::new(client, self)
    }

    /// Drive the slave group's inputs and outputs.
    ///
    /// A `SlaveGroup` will not process any inputs or outputs unless this method is called
    /// periodically. It will send an `LRW` to update slave outputs and read slave inputs.
    ///
    /// This method returns the working counter on success.
    ///
    /// # Errors
    ///
    /// This method will return with an error if the PDU could not be sent over the network, or the
    /// response times out.
    ///
    /// # Panics
    ///
    /// This method will panic if the frame data length of the group is too large to fit in the
    /// configured maximum PDU length set by the `DATA` const generic of
    /// [`PduStorage`](crate::PduStorage).
    pub async fn tx_rx<'sto>(&self, client: &'sto Client<'sto>) -> Result<u16, Error> {
        fmt::trace!(
            "Group TX/RX, start address {:#010x}, data len {}, of which read bytes: {}",
            self.inner().pdi_start.start_address,
            self.pdi().len(),
            self.read_pdi_len
        );

        assert!(
            self.len() <= client.max_frame_data(),
            "Chunked sends not yet supported. Buffer len {} B too long to send in {} B frame",
            self.len(),
            client.max_frame_data()
        );

        let data = Command::lrw(self.inner().pdi_start.start_address)
            .ignore_wkc()
            .send_receive_slice(client, self.pdi())
            .await?;

        self.process_pdi_response(data)
    }

    /// Drive the slave group's inputs and outputs and synchronise EtherCAT system time with `FRMW`.
    ///
    /// A `SlaveGroup` will not process any inputs or outputs unless this method is called
    /// periodically. It will send an `LRW` to update slave outputs and read slave inputs.
    ///
    /// This method returns the working counter and the current EtherCAT system time in nanoseconds
    /// on success.
    ///
    /// # Errors
    ///
    /// This method will return with an error if the PDU could not be sent over the network, or the
    /// response times out.
    ///
    /// # Panics
    ///
    /// This method will panic if the frame data length of the group is too large to fit in the
    /// configured maximum PDU length set by the `DATA` const generic of
    /// [`PduStorage`](crate::PduStorage).
    pub async fn tx_rx_sync_system_time<'sto>(
        &self,
        client: &'sto Client<'sto>,
    ) -> Result<(u16, Option<u64>), Error> {
        assert!(
            self.len() <= client.max_frame_data(),
            "Chunked sends not yet supported. Buffer len {} B too long to send in {} B frame",
            self.len(),
            client.max_frame_data()
        );

        fmt::trace!(
            "Group TX/RX with DC sync, start address {:#010x}, data len {}, of which read bytes: {}",
            self.inner().pdi_start.start_address,
            self.pdi().len(),
            self.read_pdi_len
        );

        if let Some(dc_ref) = client.dc_ref_address() {
            let (time, wkc) = client
                .multi_pdu(
                    |frame| {
                        let dc_handle = frame.push_pdu::<u64>(
                            Command::frmw(dc_ref, RegisterAddress::DcSystemTime.into()).into(),
                            0u64,
                            None,
                            true,
                        )?;

                        let pdu_handle = frame.push_pdu::<()>(
                            Command::lrw(self.inner().pdi_start.start_address).into(),
                            self.pdi(),
                            None,
                            false,
                        )?;

                        Ok((dc_handle, pdu_handle))
                    },
                    |received, (dc, data)| {
                        self.process_pdi_response_with_time(
                            received.take(dc)?,
                            received.take(data)?,
                        )
                    },
                )
                .await?;

            Ok((wkc, Some(time)))
        } else {
            self.tx_rx(client).await.map(|wkc| (wkc, None))
        }
    }

    fn process_pdi_response_with_time(
        &self,
        dc: crate::pdu_loop::ReceivedPdu<'_, u64>,
        data: crate::pdu_loop::ReceivedPdu<'_, ()>,
    ) -> Result<(u64, u16), Error> {
        let time = u64::unpack_from_slice(&dc)?;

        Ok((time, self.process_pdi_response(data)?))
    }

    /// Take a received PDI and copy its inputs into the group's memory.
    ///
    /// Returns working counter on success.
    fn process_pdi_response(
        &self,
        data: crate::pdu_loop::ReceivedPdu<'_, ()>,
    ) -> Result<u16, Error> {
        if data.len() != self.pdi().len() {
            fmt::error!(
                "Data length {} does not match value length {}",
                data.len(),
                self.pdi().len()
            );

            return Err(Error::Pdu(PduError::Decode));
        }

        let wkc = data.working_counter;

        self.pdi_mut()[0..self.read_pdi_len].copy_from_slice(&data[0..self.read_pdi_len]);

        Ok(wkc)
    }
}

// Methods for when the group has a PDI AND has Distributed Clocks configured
impl<const MAX_SLAVES: usize, const MAX_PDI: usize, S> SlaveGroup<MAX_SLAVES, MAX_PDI, S, HasDc>
where
    S: HasPdi,
{
    /// Drive the slave group's inputs and outputs, synchronise EtherCAT system time with `FRMW`,
    /// and return cycle timing information.
    ///
    /// A `SlaveGroup` will not process any inputs or outputs unless this method is called
    /// periodically. It will send an `LRW` to update slave outputs and read slave inputs.
    ///
    /// This method returns the working counter and a [`CycleInfo`], containing values that can be
    /// used to synchronise the MainDevice to the network SYNC0 event.
    ///
    /// ## Examples
    ///
    /// This example sends process data at 2.5ms offset into a 5ms cycle.
    ///
    /// ```rust,no_run
    /// # use ethercrab::{
    /// #     error::Error,
    /// #     slave_group::{CycleInfo, DcConfiguration},
    /// #     std::ethercat_now,
    /// #     Client, ClientConfig, PduStorage, Timeouts, DcSync,
    /// # };
    /// # use std::time::{Duration, Instant};
    /// # const MAX_SLAVES: usize = 16;
    /// # const MAX_PDU_DATA: usize = PduStorage::element_size(1100);
    /// # const MAX_FRAMES: usize = 32;
    /// # const PDI_LEN: usize = 64;
    /// # static PDU_STORAGE: PduStorage<MAX_FRAMES, MAX_PDU_DATA> = PduStorage::new();
    /// # fn main() -> Result<(), Error> { smol::block_on(async {
    /// let (_tx, _rx, pdu_loop) = PDU_STORAGE.try_split().expect("can only split once");
    ///
    /// let client = Client::new(pdu_loop, Timeouts::default(), ClientConfig::default());
    ///
    /// let cycle_time = Duration::from_millis(5);
    ///
    /// let mut group = client
    ///     .init_single_group::<MAX_SLAVES, PDI_LEN>(ethercat_now)
    ///     .await
    ///     .expect("Init");
    ///
    /// // This example enables SYNC0 for every detected SubDevice
    /// for mut slave in group.iter(&client) {
    ///     slave.set_dc_sync(DcSync::Sync0);
    /// }
    ///
    /// let group = group
    ///     .into_pre_op_pdi(&client)
    ///     .await
    ///     .expect("PRE-OP -> PRE-OP with PDI")
    ///     .configure_dc_sync(
    ///         &client,
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
    ///     .request_into_op(&client)
    ///     .await
    ///     .expect("PRE-OP -> SAFE-OP -> OP");
    ///
    /// // Wait for all SubDevices in the group to reach OP, whilst sending PDI to allow DC to start
    /// // correctly.
    /// while !group.all_op(&client).await? {
    ///     let now = Instant::now();
    ///
    ///     let (
    ///         _wkc,
    ///         CycleInfo {
    ///             next_cycle_wait, ..
    ///         },
    ///     ) = group.tx_rx_dc(&client).await.expect("TX/RX");
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
    ///     ) = group.tx_rx_dc(&client).await.expect("TX/RX");
    ///
    ///     // Process data computations happen here
    ///
    ///     smol::Timer::at(now + next_cycle_wait).await;
    /// }
    /// # }) }
    /// ```
    pub async fn tx_rx_dc<'sto>(
        &self,
        client: &'sto Client<'sto>,
    ) -> Result<(u16, CycleInfo), Error> {
        assert!(
            self.len() <= client.max_frame_data(),
            "Chunked sends not yet supported. Buffer len {} B too long to send in {} B frame",
            self.len(),
            client.max_frame_data()
        );

        fmt::trace!(
            "Group TX/RX with DC sync, start address {:#010x}, data len {}, of which read bytes: {}",
            self.inner().pdi_start.start_address,
            self.pdi().len(),
            self.read_pdi_len
        );

        let (time, wkc) = client
            .multi_pdu(
                |frame| {
                    let dc_handle = frame.push_pdu::<u64>(
                        Command::frmw(self.dc_conf.reference, RegisterAddress::DcSystemTime.into())
                            .into(),
                        0u64,
                        None,
                        true,
                    )?;

                    let pdu_handle = frame.push_pdu::<()>(
                        Command::lrw(self.inner().pdi_start.start_address).into(),
                        self.pdi(),
                        None,
                        false,
                    )?;

                    Ok((dc_handle, pdu_handle))
                },
                |received, (dc, data)| {
                    self.process_pdi_response_with_time(received.take(dc)?, received.take(data)?)
                },
            )
            .await?;

        // Nanoseconds from the start of the cycle. This works because the first SYNC0 pulse
        // time is rounded to a whole number of `sync0_period`-length cycles.
        let cycle_start_offset = time % self.dc_conf.sync0_period;

        let time_to_next_iter =
            self.dc_conf.sync0_period + (self.dc_conf.sync0_shift - cycle_start_offset);

        Ok((
            wkc,
            CycleInfo {
                dc_system_time: time,
                cycle_start_offset: Duration::from_nanos(cycle_start_offset),
                next_cycle_wait: Duration::from_nanos(time_to_next_iter),
            },
        ))
    }
}
