use crate::{error::Error, subdevice_group::SubDeviceGroupRef, GroupId, SubDevice, SubDeviceGroup};
use atomic_refcell::AtomicRefCell;

/// A trait implemented only by [`SubDeviceGroup`] so multiple groups with different const params
/// can be stored in a hashmap, `Vec`, etc.
#[doc(hidden)]
#[sealed::sealed]
pub trait SubDeviceGroupHandle {
    /// Get the group's ID.
    fn id(&self) -> GroupId;

    /// Add a SubDevice device to this group.
    unsafe fn push(&self, subdevice: SubDevice) -> Result<(), Error>;

    /// Get a reference to the group with const generic params erased.
    fn as_ref(&self) -> SubDeviceGroupRef<'_>;
}

#[sealed::sealed]
impl<const MAX_SUBDEVICES: usize, const MAX_PDI: usize, S> SubDeviceGroupHandle
    for SubDeviceGroup<MAX_SUBDEVICES, MAX_PDI, S>
{
    fn id(&self) -> GroupId {
        self.id
    }

    unsafe fn push(&self, subdevice: SubDevice) -> Result<(), Error> {
        (*self.inner.get())
            .subdevices
            .push(AtomicRefCell::new(subdevice))
            .map_err(|_| Error::Capacity(crate::error::Item::SubDevice))
    }

    fn as_ref(&self) -> SubDeviceGroupRef<'_> {
        SubDeviceGroupRef::new(self)
    }
}
