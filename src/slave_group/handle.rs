use crate::{error::Error, slave_group::SlaveGroupRef, GroupId, Slave, SlaveGroup};
use atomic_refcell::AtomicRefCell;

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
