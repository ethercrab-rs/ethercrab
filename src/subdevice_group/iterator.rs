use super::{HasPdi, PreOp};
use crate::{fmt, Client, SubDevice, SubDeviceGroup, SubDevicePdi, SubDeviceRef};
use atomic_refcell::AtomicRefMut;

/// An iterator over all SubDevices in a group.
///
/// Created by calling [`SubDeviceGroup::iter`](crate::subdevice_group::SubDeviceGroup::iter).
pub struct GroupSubDeviceIterator<
    'group,
    'client,
    const MAX_SUBDEVICES: usize,
    const MAX_PDI: usize,
    S,
    DC,
> {
    group: &'group SubDeviceGroup<MAX_SUBDEVICES, MAX_PDI, S, DC>,
    idx: usize,
    client: &'client Client<'client>,
}

impl<'group, 'client, const MAX_SUBDEVICES: usize, const MAX_PDI: usize, S, DC>
    GroupSubDeviceIterator<'group, 'client, MAX_SUBDEVICES, MAX_PDI, S, DC>
{
    pub(in crate::subdevice_group) fn new(
        client: &'client Client<'client>,
        group: &'group SubDeviceGroup<MAX_SUBDEVICES, MAX_PDI, S, DC>,
    ) -> Self {
        Self {
            group,
            idx: 0,
            client,
        }
    }
}

// Impl for SubDevices that don't have a PDI yet
impl<'group, 'client, const MAX_SUBDEVICES: usize, const MAX_PDI: usize, DC> Iterator
    for GroupSubDeviceIterator<'group, 'client, MAX_SUBDEVICES, MAX_PDI, PreOp, DC>
where
    'client: 'group,
{
    type Item = SubDeviceRef<'group, AtomicRefMut<'group, SubDevice>>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.idx >= self.group.len() {
            return None;
        }

        let subdevice = fmt::unwrap!(self.group.subdevice(self.client, self.idx).map_err(|e| {
            fmt::error!("Failed to get SubDevice at index {} from group with {} SubDevices: {}. This is very wrong. Please open an issue.", self.idx, self.group.len(), e);

            e
        }));

        self.idx += 1;

        Some(subdevice)
    }
}

// Impl for SubDevices with PDI
impl<'group, 'client, const MAX_SUBDEVICES: usize, const MAX_PDI: usize, S, DC> Iterator
    for GroupSubDeviceIterator<'group, 'client, MAX_SUBDEVICES, MAX_PDI, S, DC>
where
    'client: 'group,
    S: HasPdi,
{
    type Item = SubDeviceRef<'group, SubDevicePdi<'group>>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.idx >= self.group.len() {
            return None;
        }

        let subdevice = fmt::unwrap!(self.group.subdevice(self.client, self.idx).map_err(|e| {
            fmt::error!("Failed to get SubDevice at index {} from group with {} SubDevices: {}. This is very wrong. Please open an issue.", self.idx, self.group.len(), e);

            e
        }));

        self.idx += 1;

        Some(subdevice)
    }
}
