//! A group of slave devices.
//!
//! Slaves can be divided into multiple groups to allow multiple tasks to run concurrently,
//! potentially at different tick rates.

mod configurator;

use crate::{
    error::{Error, Item},
    pdi::PdiOffset,
    slave::{configuration::PdoDirection, pdi::SlavePdi, IoRanges, Slave, SlaveRef},
    timer_factory::timeout,
    Client, SlaveState,
};
use core::{
    cell::UnsafeCell, future::Future, marker::PhantomData, slice, sync::atomic::AtomicUsize,
};

use atomic_refcell::{AtomicRef, AtomicRefCell};
pub use configurator::SlaveGroupRef;

static GROUP_ID: AtomicUsize = AtomicUsize::new(0);

/// A group's unique ID.
#[doc(hidden)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct GroupId(usize);

impl From<GroupId> for usize {
    fn from(value: GroupId) -> Self {
        value.0
    }
}

/// A typestate for [`SlaveGroup`] representing a group that is undergoing initialisation.
///
/// This corresponds to the EtherCAT states INIT and PRE-OP.
#[derive(Copy, Clone, Debug)]
pub struct Init;

/// A typestate for [`SlaveGroup`] representing a group that is in SAFE-OP.
#[derive(Copy, Clone, Debug)]
pub struct SafeOp;

/// A typestate for [`SlaveGroup`] representing a group that is in OP.
#[derive(Copy, Clone, Debug)]
pub struct Op;

/// A group of one or more EtherCAT slaves.
///
/// Groups are created during EtherCrab initialisation, and are the only way to access individual
/// slave PDI sections.
pub struct SlaveGroup<const MAX_SLAVES: usize, const MAX_PDI: usize, S> {
    id: GroupId,
    pdi: UnsafeCell<[u8; MAX_PDI]>,
    /// The number of bytes at the beginning of the PDI reserved for slave inputs.
    read_pdi_len: usize,
    /// The total length (I and O) of the PDI for this group.
    pdi_len: usize,
    inner: UnsafeCell<GroupInner<MAX_SLAVES>>,
    _state: PhantomData<S>,
}

#[derive(Default)]
struct GroupInner<const MAX_SLAVES: usize> {
    slaves: heapless::Vec<AtomicRefCell<Slave>, MAX_SLAVES>,
    pdi_start: PdiOffset,
}

impl<const MAX_SLAVES: usize, const MAX_PDI: usize> SlaveGroup<MAX_SLAVES, MAX_PDI, Init> {
    /// Transition slaves in this group from PRE-OP to SAFE-OP.
    pub async fn into_safe_op<F, O>(
        mut self,
        client: &Client<'_>,
        mut preop_safeop_hook: F,
    ) -> Result<SlaveGroup<MAX_SLAVES, MAX_PDI, SafeOp>, Error>
    where
        F: FnMut(SlaveRef<'_, &Slave>) -> O,
        O: Future<Output = Result<(), Error>>,
    {
        let mut inner = self.inner.into_inner();

        let mut pdi_position = inner.pdi_start;

        log::debug!(
            "Going to configure group with {} slave(s), starting PDI offset {:#010x}",
            inner.slaves.len(),
            inner.pdi_start.start_address
        );

        // Slaves must be in PRE-OP at this point.

        // Configure master read PDI mappings in the first section of the PDI
        for slave in inner.slaves.iter_mut() {
            let configured_address = slave.get_mut().configured_address;

            let fut =
                (preop_safeop_hook)(SlaveRef::new(client, configured_address, slave.get_mut()));

            fut.await?;

            // We're in PRE-OP at this point
            pdi_position = SlaveRef::new(client, configured_address, slave.get_mut())
                .configure_fmmus(
                    pdi_position,
                    inner.pdi_start.start_address,
                    PdoDirection::MasterRead,
                )
                .await?;
        }

        self.read_pdi_len = (pdi_position.start_address - inner.pdi_start.start_address) as usize;

        log::debug!("Slave mailboxes configured and init hooks called");

        // We configured all read PDI mappings as a contiguous block in the previous loop. Now we'll
        // configure the write mappings in a separate loop. This means we have IIIIOOOO instead of
        // IOIOIO.
        for (_i, slave) in inner.slaves.iter_mut().enumerate() {
            let slave = slave.get_mut();

            let addr = slave.configured_address;
            let _name = slave.name.clone();

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

        log::debug!("Slave FMMUs configured for group. Able to move to SAFE-OP");

        let pdi_len = (pdi_position.start_address - inner.pdi_start.start_address) as usize;

        log::debug!(
            "Group PDI length: start {:#010x}, {} total bytes ({} input bytes)",
            inner.pdi_start.start_address,
            pdi_len,
            self.read_pdi_len
        );

        if pdi_len > MAX_PDI {
            return Err(Error::PdiTooLong {
                max_length: MAX_PDI,
                desired_length: pdi_len,
            });
        }

        self.pdi_len = pdi_len;

        // We're done configuring FMMUs, etc, now we can request all slaves in this group go into
        // SAFE-OP
        for (_i, slave) in inner.slaves.iter_mut().enumerate() {
            let slave = slave.get_mut();

            SlaveRef::new(client, slave.configured_address, slave)
                .request_safe_op_nowait()
                .await?;
        }

        // Wait for everything to go into PRE-OP
        timeout(client.timeouts.state_transition, async {
            loop {
                let mut all_transitioned = true;

                for (_i, slave) in inner.slaves.iter_mut().enumerate() {
                    let slave = slave.get_mut();

                    // TODO: Add a way to queue up a bunch of PDUs and send all at once
                    let (slave_state, _al_status_code) =
                        SlaveRef::new(client, slave.configured_address, slave)
                            .status()
                            .await?;

                    if slave_state != SlaveState::SafeOp {
                        all_transitioned = false;
                    }
                }

                if all_transitioned {
                    break Ok(());
                }

                client.timeouts.loop_tick().await;
            }
        })
        .await?;

        Ok(SlaveGroup {
            id: self.id,
            pdi: self.pdi,
            read_pdi_len: self.read_pdi_len,
            pdi_len: self.pdi_len,
            inner: UnsafeCell::new(inner),
            _state: PhantomData,
        })
    }
}

impl<const MAX_SLAVES: usize, const MAX_PDI: usize> SlaveGroup<MAX_SLAVES, MAX_PDI, SafeOp> {
    /// Transition all slave devices in the group from SAFE-OP to OP.
    pub async fn into_op(self) -> Result<SlaveGroup<MAX_SLAVES, MAX_PDI, Op>, Error> {
        // TODO: Put slaves into OP, wait for them

        Ok(SlaveGroup {
            id: self.id,
            pdi: self.pdi,
            read_pdi_len: self.read_pdi_len,
            pdi_len: self.pdi_len,
            inner: self.inner,
            _state: PhantomData,
        })
    }
}

// FIXME: Remove these unsafe impls if possible. There's some weird quirkiness when moving a group
// into an async block going on...
// TODO: Can we constrain the typestate here to just the one(s) that need to be?
unsafe impl<const MAX_SLAVES: usize, const MAX_PDI: usize, S> Sync
    for SlaveGroup<MAX_SLAVES, MAX_PDI, S>
{
}
unsafe impl<const MAX_SLAVES: usize, const MAX_PDI: usize, S> Send
    for SlaveGroup<MAX_SLAVES, MAX_PDI, S>
{
}

/// A trait implemented only by [`SlaveGroup`] so multiple groups with different const params can be
/// stored in a hashmap, `Vec`, etc.
#[doc(hidden)]
#[sealed::sealed]
pub trait SlaveGroupHandle {
    /// Get the group's ID.
    fn id(&self) -> GroupId;

    /// Add a slave device to this group.
    unsafe fn push(&self, slave: Slave) -> Result<(), Error>;

    /// Get a reference to the group with const generic params erased.
    fn as_ref(&self) -> SlaveGroupRef<'_>;
}

#[sealed::sealed]
impl<const MAX_SLAVES: usize, const MAX_PDI: usize, S> SlaveGroupHandle
    for SlaveGroup<MAX_SLAVES, MAX_PDI, S>
{
    fn id(&self) -> GroupId {
        self.id
    }

    unsafe fn push(&self, slave: Slave) -> Result<(), Error> {
        (*self.inner.get())
            .slaves
            .push(AtomicRefCell::new(slave))
            .map_err(|_| Error::Capacity(crate::error::Item::Slave))
    }

    fn as_ref(&self) -> SlaveGroupRef<'_> {
        SlaveGroupRef::new(self)
    }
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
    /// Create a new slave group with a given PRE OP -> SAFE OP hook.
    ///
    /// The hook can be used to configure slaves using SDOs.
    pub fn new() -> Self {
        Self::default()
    }

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

    /// Get an iterator over all slaves in this group.
    pub fn iter<'group, 'client>(
        &'group mut self,
        client: &'client Client<'client>,
    ) -> GroupSlaveIterator<'group, 'client, MAX_SLAVES, MAX_PDI, S> {
        GroupSlaveIterator {
            group: self,
            idx: 0,
            client,
        }
    }

    /// Retrieve a reference to a slave in this group by index.
    ///
    /// Each slave may not be individually borrowed more than once, but multiple slaves can be
    /// borrowed at the same time. If the slave at the given index is already borrowed, this method
    /// will return an [`Error::Borrow`].
    pub fn slave<'client>(
        &self,
        client: &'client Client<'client>,
        index: usize,
    ) -> Result<SlaveRef<'client, SlavePdi<'_>>, Error> {
        let slave = self
            .inner()
            .slaves
            .get(index)
            .ok_or(Error::NotFound {
                item: Item::Slave,
                index: Some(index),
            })?
            .try_borrow_mut()
            .map_err(|e| {
                log::error!("Slave index {}: {}", index, e);

                Error::Borrow
            })?;

        let IoRanges {
            input: input_range,
            output: output_range,
        } = slave.io_segments();

        // SAFETY: Multiple references are ok as long as I and O ranges do not overlap.
        let i_data = self.pdi();
        let o_data = self.pdi_mut();

        log::trace!(
            "Get slave {:#06x} IO ranges I: {}, O: {}",
            slave.configured_address,
            input_range,
            output_range
        );

        log::trace!(
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

    fn pdi_mut(&self) -> &mut [u8] {
        let all_buf = unsafe { &mut *self.pdi.get() };

        &mut all_buf[0..self.pdi_len]
    }

    fn pdi(&self) -> &[u8] {
        let all_buf = unsafe { &*self.pdi.get() };

        &all_buf[0..self.pdi_len]
    }

    /// Drive the slave group's inputs and outputs.
    ///
    /// A `SlaveGroup` will not process any inputs or outputs unless this method is called
    /// periodically. It will send an `LRW` to update slave outputs and read slave inputs.
    pub async fn tx_rx<'sto>(&self, client: &'sto Client<'sto>) -> Result<(), Error> {
        log::trace!(
            "Group TX/RX, start address {:#010x}, data len {}, of which read bytes: {}",
            self.inner().pdi_start.start_address,
            self.pdi_mut().len(),
            self.read_pdi_len
        );

        let (_res, _wkc) = client
            .lrw_buf(
                self.inner().pdi_start.start_address,
                self.pdi_mut(),
                self.read_pdi_len,
            )
            .await?;

        Ok(())

        // FIXME: EL400 gives 2, expects 3
        // if wkc != self.group_working_counter {
        //     Err(Error::WorkingCounter {
        //         expected: self.group_working_counter,
        //         received: wkc,
        //         context: Some("group working counter"),
        //     })
        // } else {
        //     Ok(())
        // }
    }
}

/// An iterator over all slaves in a group.
///
/// Created by calling [`SlaveGroup::iter`](crate::slave_group::SlaveGroup::iter).
pub struct GroupSlaveIterator<'group, 'client, const MAX_SLAVES: usize, const MAX_PDI: usize, S> {
    group: &'group SlaveGroup<MAX_SLAVES, MAX_PDI, S>,
    idx: usize,
    client: &'client Client<'client>,
}

impl<'group, 'client, const MAX_SLAVES: usize, const MAX_PDI: usize, S> Iterator
    for GroupSlaveIterator<'group, 'client, MAX_SLAVES, MAX_PDI, S>
where
    'client: 'group,
{
    type Item = SlaveRef<'group, SlavePdi<'group>>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.idx >= self.group.len() {
            return None;
        }

        // Squelch errors. If we're failing at this point, something is _very_ wrong.
        let slave = self.group.slave(self.client, self.idx).map_err(|e| {
            log::error!("Failed to get slave at index {} from group with {} slaves: {e:?}. This is very wrong. Please open an issue.", self.idx, self.group.len());
        }).ok()?;

        self.idx += 1;

        Some(slave)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn group_unique_id_defaults() {
        let g1 = SlaveGroup::<16, 16, Init>::default();
        let g2 = SlaveGroup::<16, 16, Init>::default();
        let g3 = SlaveGroup::<16, 16, Init>::default();

        assert_ne!(g1.id, g2.id);
        assert_ne!(g2.id, g3.id);
        assert_ne!(g1.id, g3.id);
    }

    #[test]
    fn group_unique_id_same_fn() {
        let g1 = SlaveGroup::<16, 16, Init>::new();
        let g2 = SlaveGroup::<16, 16, Init>::new();

        assert_ne!(g1.id, g2.id);
    }
}
