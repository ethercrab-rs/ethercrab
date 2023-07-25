mod configurator;

use crate::{
    error::{Error, Item},
    slave::{pdi::SlavePdi, IoRanges, Slave, SlaveRef},
    Client,
};
use core::{cell::UnsafeCell, future::Future, pin::Pin, slice, sync::atomic::AtomicUsize};

#[cfg(not(feature = "std"))]
use alloc::boxed::Box;

use atomic_refcell::AtomicRefCell;
pub use configurator::SlaveGroupRef;

// TODO: When the right async-trait stuff is stabilised, it should be possible to remove the
// `Box`ing here, and make this work without an allocator. See also
// <https://users.rust-lang.org/t/store-async-closure-on-struct-in-no-std/82929>
type HookFuture<'any> = Pin<Box<dyn Future<Output = Result<(), Error>> + Send + 'any>>;

pub(crate) type HookFn = for<'any> fn(&'any SlaveRef<'any, &'any mut Slave>) -> HookFuture<'any>;

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

/// A group of one or more EtherCAT slaves.
///
/// Groups are created during EtherCrab initialisation, and are the only way to access individual
/// slave PDI sections.
pub struct SlaveGroup<const MAX_SLAVES: usize, const MAX_PDI: usize> {
    id: GroupId,
    pdi: UnsafeCell<[u8; MAX_PDI]>,
    preop_safeop_hook: Option<HookFn>,
    inner: UnsafeCell<GroupInner<MAX_SLAVES>>,
}

#[derive(Default)]
struct GroupInner<const MAX_SLAVES: usize> {
    slaves: heapless::Vec<AtomicRefCell<Slave>, MAX_SLAVES>,

    /// The number of bytes at the beginning of the PDI reserved for slave inputs.
    read_pdi_len: usize,
    /// The total length (I and O) of the PDI for this group.
    pdi_len: usize,
    start_address: u32,
}

// FIXME: Remove these unsafe impls if possible. There's some weird quirkiness when moving a group
// into an async block going on...
unsafe impl<const MAX_SLAVES: usize, const MAX_PDI: usize> Sync
    for SlaveGroup<MAX_SLAVES, MAX_PDI>
{
}
unsafe impl<const MAX_SLAVES: usize, const MAX_PDI: usize> Send
    for SlaveGroup<MAX_SLAVES, MAX_PDI>
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
impl<const MAX_SLAVES: usize, const MAX_PDI: usize> SlaveGroupHandle
    for SlaveGroup<MAX_SLAVES, MAX_PDI>
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

impl<const MAX_SLAVES: usize, const MAX_PDI: usize> Default for SlaveGroup<MAX_SLAVES, MAX_PDI> {
    fn default() -> Self {
        Self {
            id: GroupId(GROUP_ID.fetch_add(1, core::sync::atomic::Ordering::Relaxed)),
            pdi: UnsafeCell::new([0u8; MAX_PDI]),
            preop_safeop_hook: Default::default(),
            inner: UnsafeCell::new(GroupInner::default()),
        }
    }
}

/// Returned when a slave device's input or output PDI segment is empty.
static EMPTY_PDI_SLICE: &[u8] = &[];

impl<const MAX_SLAVES: usize, const MAX_PDI: usize> SlaveGroup<MAX_SLAVES, MAX_PDI> {
    /// Create a new slave group with a given PRE OP -> SAFE OP hook.
    ///
    /// The hook can be used to configure slaves using SDOs.
    pub fn new(preop_safeop_hook: HookFn) -> Self {
        Self {
            preop_safeop_hook: Some(preop_safeop_hook),
            ..Default::default()
        }
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
    ) -> GroupSlaveIterator<'group, 'client, MAX_SLAVES, MAX_PDI> {
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
            self.inner().pdi_len,
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

        &mut all_buf[0..self.inner().pdi_len]
    }

    fn pdi(&self) -> &[u8] {
        let all_buf = unsafe { &*self.pdi.get() };

        &all_buf[0..self.inner().pdi_len]
    }

    /// Drive the slave group's inputs and outputs.
    ///
    /// A `SlaveGroup` will not process any inputs or outputs unless this method is called
    /// periodically. It will send an `LRW` to update slave outputs and read slave inputs.
    pub async fn tx_rx<'sto>(&self, client: &'sto Client<'sto>) -> Result<(), Error> {
        log::trace!(
            "Group TX/RX, start address {:#010x}, data len {}, of which read bytes: {}",
            self.inner().start_address,
            self.pdi_mut().len(),
            self.inner().read_pdi_len
        );

        let (_res, _wkc) = client
            .lrw_buf(
                self.inner().start_address,
                self.pdi_mut(),
                self.inner().read_pdi_len,
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
pub struct GroupSlaveIterator<'group, 'client, const MAX_SLAVES: usize, const MAX_PDI: usize> {
    group: &'group SlaveGroup<MAX_SLAVES, MAX_PDI>,
    idx: usize,
    client: &'client Client<'client>,
}

impl<'group, 'client, const MAX_SLAVES: usize, const MAX_PDI: usize> Iterator
    for GroupSlaveIterator<'group, 'client, MAX_SLAVES, MAX_PDI>
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
        let g1 = SlaveGroup::<16, 16>::default();
        let g2 = SlaveGroup::<16, 16>::default();
        let g3 = SlaveGroup::<16, 16>::default();

        assert_ne!(g1.id, g2.id);
        assert_ne!(g2.id, g3.id);
        assert_ne!(g1.id, g3.id);
    }

    #[test]
    fn group_unique_id_same_fn() {
        let g1 = SlaveGroup::<16, 16>::new(|_| Box::pin(async { Ok(()) }));
        let g2 = SlaveGroup::<16, 16>::new(|_| Box::pin(async { Ok(()) }));

        assert_ne!(g1.id, g2.id);
    }
}
