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
    error::{Error, Item},
    fmt,
    pdi::PdiOffset,
    slave::{configuration::PdoDirection, pdi::SlavePdi, IoRanges, Slave, SlaveRef},
    timer_factory::IntoTimeout,
    Client, RegisterAddress, SlaveState,
};
use atomic_refcell::{AtomicRefCell, AtomicRefMut};
use core::{cell::UnsafeCell, marker::PhantomData, slice, sync::atomic::AtomicUsize};

pub use self::group_id::GroupId;
pub use self::handle::SlaveGroupHandle;
pub use self::iterator::GroupSlaveIterator;
pub use configurator::SlaveGroupRef;

static GROUP_ID: AtomicUsize = AtomicUsize::new(0);

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

#[derive(Default)]
struct GroupInner<const MAX_SLAVES: usize> {
    slaves: heapless::Vec<AtomicRefCell<Slave>, MAX_SLAVES>,
    pdi_start: PdiOffset,
}

/// A group of one or more EtherCAT slaves.
///
/// Groups are created during EtherCrab initialisation, and are the only way to access individual
/// slave PDI sections.
pub struct SlaveGroup<const MAX_SLAVES: usize, const MAX_PDI: usize, S = PreOp> {
    id: GroupId,
    pdi: UnsafeCell<[u8; MAX_PDI]>,
    /// The number of bytes at the beginning of the PDI reserved for slave inputs.
    read_pdi_len: usize,
    /// The total length (I and O) of the PDI for this group.
    pdi_len: usize,
    inner: UnsafeCell<GroupInner<MAX_SLAVES>>,
    _state: PhantomData<S>,
}

impl<const MAX_SLAVES: usize, const MAX_PDI: usize> SlaveGroup<MAX_SLAVES, MAX_PDI, PreOp> {
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

            // TODO: DC active sync/events/SYNC0/etc
            // // FIXME: Just first slave or all slaves?
            // // if name == "EL2004" {
            // // if i == 0 {
            // if false {
            //     log::info!("Slave {:#06x} {} DC", addr, name);
            //     // let slave_config = SlaveRef::new(client, slave.configured_address, ());

            //     // TODO: Pass in as config
            //     let cycle_time = Duration::from_millis(2).as_nanos() as u32;

            //     // Disable sync signals
            //     slave_config
            //         .write(RegisterAddress::DcSyncActive, 0x00u8, "disable sync")
            //         .await?;

            //     let local_time: u32 = slave_config
            //         .read(RegisterAddress::DcSystemTime, "local time")
            //         .await?;

            //     // TODO: Pass in as config
            //     // let startup_delay = Duration::from_millis(100).as_nanos() as u32;
            //     let startup_delay = 0;

            //     // TODO: Pass in as config
            //     let start_time = local_time + cycle_time + startup_delay;

            //     slave_config
            //         .write(
            //             RegisterAddress::DcSyncStartTime,
            //             start_time,
            //             "sync start time",
            //         )
            //         .await?;

            //     slave_config
            //         .write(
            //             RegisterAddress::DcSync0CycleTime,
            //             cycle_time,
            //             "sync cycle time",
            //         )
            //         .await?;

            //     // Enable cyclic operation (0th bit) and sync0 signal (1st bit)
            //     slave_config
            //         .write(RegisterAddress::DcSyncActive, 0b11u8, "enable sync0")
            //         .await?;
            // }
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

    /// Get an iterator over all slaves in this group.
    pub fn iter<'group, 'client>(
        &'group mut self,
        client: &'client Client<'client>,
    ) -> GroupSlaveIterator<'group, 'client, MAX_SLAVES, MAX_PDI, Self> {
        GroupSlaveIterator::new(client, self)
    }

    /// Transition the group from PRE-OP -> SAFE-OP -> OP.
    ///
    /// To transition individually from PRE-OP to SAFE-OP, then SAFE-OP to OP, see
    /// [`SlaveGroup::into_safe_op`].
    pub async fn into_op(
        self,
        client: &Client<'_>,
    ) -> Result<SlaveGroup<MAX_SLAVES, MAX_PDI, Op>, Error> {
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
    ) -> Result<SlaveGroup<MAX_SLAVES, MAX_PDI, PreOpPdi>, Error> {
        self.configure_fmmus(client).await?;

        Ok(SlaveGroup {
            id: self.id,
            pdi: self.pdi,
            read_pdi_len: self.read_pdi_len,
            pdi_len: self.pdi_len,
            inner: UnsafeCell::new(self.inner.into_inner()),
            _state: PhantomData,
        })
    }

    /// Transition the slave group from PRE-OP to SAFE-OP.
    pub async fn into_safe_op(
        self,
        client: &Client<'_>,
    ) -> Result<SlaveGroup<MAX_SLAVES, MAX_PDI, SafeOp>, Error> {
        let self_ = self.into_pre_op_pdi(client).await?;

        // We're done configuring FMMUs, etc, now we can request all slaves in this group go into
        // SAFE-OP
        self_.transition_to(client, SlaveState::SafeOp).await
    }
}

impl<const MAX_SLAVES: usize, const MAX_PDI: usize> SlaveGroup<MAX_SLAVES, MAX_PDI, PreOpPdi> {
    /// Transition the slave group from PRE-OP to SAFE-OP.
    pub async fn into_safe_op(
        self,
        client: &Client<'_>,
    ) -> Result<SlaveGroup<MAX_SLAVES, MAX_PDI, SafeOp>, Error> {
        self.transition_to(client, SlaveState::SafeOp).await
    }

    /// Transition all slave devices in the group from PRE-OP to SAFE-OP.
    pub async fn into_op(
        self,
        client: &Client<'_>,
    ) -> Result<SlaveGroup<MAX_SLAVES, MAX_PDI, Op>, Error> {
        self.transition_to(client, SlaveState::Op).await
    }
}

impl<const MAX_SLAVES: usize, const MAX_PDI: usize> SlaveGroup<MAX_SLAVES, MAX_PDI, SafeOp> {
    /// Transition all slave devices in the group from SAFE-OP to OP.
    pub async fn into_op(
        self,
        client: &Client<'_>,
    ) -> Result<SlaveGroup<MAX_SLAVES, MAX_PDI, Op>, Error> {
        self.transition_to(client, SlaveState::Op).await
    }
}

unsafe impl<const MAX_SLAVES: usize, const MAX_PDI: usize, S> Sync
    for SlaveGroup<MAX_SLAVES, MAX_PDI, S>
{
}
unsafe impl<const MAX_SLAVES: usize, const MAX_PDI: usize, S> Send
    for SlaveGroup<MAX_SLAVES, MAX_PDI, S>
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
            _state: PhantomData,
        }
    }
}

/// Returned when a slave device's input or output PDI segment is empty.
static EMPTY_PDI_SLICE: &[u8] = &[];

impl<const MAX_SLAVES: usize, const MAX_PDI: usize, S> SlaveGroup<MAX_SLAVES, MAX_PDI, S> {
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

    /// Wait for all slaves in this group to transition to the given state.
    async fn wait_for_state(
        &self,
        client: &Client<'_>,
        desired_state: SlaveState,
    ) -> Result<(), Error> {
        async {
            loop {
                let mut all_transitioned = true;

                for slave in self.inner().slaves.iter().map(|slave| slave.borrow()) {
                    // TODO: Add a way to queue up a bunch of PDUs and send all at once
                    let slave_state = SlaveRef::new(client, slave.configured_address, slave)
                        .state()
                        .await?;

                    if slave_state != desired_state {
                        all_transitioned = false;
                    }
                }

                if all_transitioned {
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
    ) -> Result<SlaveGroup<MAX_SLAVES, MAX_PDI, TO>, Error> {
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
            _state: PhantomData,
        })
    }
}

/// Items common to all states ([`PreOp`], [`Op`], etc) on [`SlaveGroup`].
///
/// This trait is sealed and may not be implemented on types external to EtherCrab.
#[sealed::sealed]
pub trait SlaveGroupState {
    /// The type of state returned with the [`SlaveGroup`] from the
    /// [`slave`](SlaveGroupState::slave) method, e.g. [`SlavePdi`](crate::SlavePdi), etc.
    type RefType<'group>
    where
        Self: 'group;

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
    fn slave<'client, 'group>(
        &'group self,
        client: &'client Client<'client>,
        index: usize,
    ) -> Result<SlaveRef<'client, Self::RefType<'group>>, Error>;

    /// Returns `true` if there are no slave devices in the group.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Get the number of slave devices in this group.
    fn len(&self) -> usize;
}

#[sealed::sealed]
impl<const MAX_SLAVES: usize, const MAX_PDI: usize> SlaveGroupState
    for SlaveGroup<MAX_SLAVES, MAX_PDI, PreOp>
{
    type RefType<'group> = AtomicRefMut<'group, Slave>;

    fn slave<'client, 'group>(
        &'group self,
        client: &'client Client<'client>,
        index: usize,
    ) -> Result<SlaveRef<'client, Self::RefType<'group>>, Error> {
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

    fn len(&self) -> usize {
        self.len()
    }
}

#[doc(hidden)]
pub trait HasPdi {}

impl HasPdi for PreOpPdi {}
impl HasPdi for SafeOp {}
impl HasPdi for Op {}

#[sealed::sealed]
impl<const MAX_SLAVES: usize, const MAX_PDI: usize, S> SlaveGroupState
    for SlaveGroup<MAX_SLAVES, MAX_PDI, S>
where
    S: HasPdi,
{
    type RefType<'group> = SlavePdi<'group> where S: 'group;

    fn slave<'client, 'group>(
        &'group self,
        client: &'client Client<'client>,
        index: usize,
    ) -> Result<SlaveRef<'client, Self::RefType<'group>>, Error> {
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

    fn len(&self) -> usize {
        self.len()
    }
}

// Methods for any state where a PDI has been configured.
impl<const MAX_SLAVES: usize, const MAX_PDI: usize, S> SlaveGroup<MAX_SLAVES, MAX_PDI, S>
where
    S: HasPdi,
{
    /// Get an iterator over all slaves in this group.
    pub fn iter<'group, 'client>(
        &'group mut self,
        client: &'client Client<'client>,
    ) -> GroupSlaveIterator<'group, 'client, MAX_SLAVES, MAX_PDI, Self> {
        GroupSlaveIterator::new(client, self)
    }

    /// Drive the slave group's inputs and outputs.
    ///
    /// A `SlaveGroup` will not process any inputs or outputs unless this method is called
    /// periodically. It will send an `LRW` to update slave outputs and read slave inputs.
    ///
    /// This method returns the working counter on success.
    pub async fn tx_rx<'sto>(&self, client: &'sto Client<'sto>) -> Result<u16, Error> {
        fmt::trace!(
            "Group TX/RX, start address {:#010x}, data len {}, of which read bytes: {}",
            self.inner().pdi_start.start_address,
            self.pdi().len(),
            self.read_pdi_len
        );

        if let Some(dc_ref) = client.dc_ref_address() {
            let (_, (_res, wkc)) = futures_lite::future::try_zip(
                // TODO: Store time in group so we can figure out when to send PDI if DC is in use.
                Command::frmw(dc_ref, RegisterAddress::DcSystemTime.into())
                    .wrap(client)
                    .ignore_wkc()
                    // TODO
                    // .with_wkc(expected_dc_wkc)
                    .receive::<u64>(),
                Command::lrw(self.inner().pdi_start.start_address)
                    .wrap(client)
                    .send_receive_slice_mut(self.pdi_mut(), self.read_pdi_len),
            )
            .await?;

            Ok(wkc)
        } else {
            let (_res, wkc) = Command::lrw(self.inner().pdi_start.start_address)
                .wrap(client)
                .send_receive_slice_mut(self.pdi_mut(), self.read_pdi_len)
                .await?;

            Ok(wkc)
        }
    }
}
