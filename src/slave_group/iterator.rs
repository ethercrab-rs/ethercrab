use super::{HasPdi, PreOp};
use crate::{fmt, Client, Slave, SlaveGroup, SlavePdi, SlaveRef};
use atomic_refcell::AtomicRefMut;

/// An iterator over all slaves in a group.
///
/// Created by calling [`SlaveGroup::iter`](crate::slave_group::SlaveGroup::iter).
pub struct GroupSlaveIterator<'group, 'client, const MAX_SLAVES: usize, const MAX_PDI: usize, S, DC>
{
    group: &'group SlaveGroup<MAX_SLAVES, MAX_PDI, S, DC>,
    idx: usize,
    client: &'client Client<'client>,
}

impl<'group, 'client, const MAX_SLAVES: usize, const MAX_PDI: usize, S, DC>
    GroupSlaveIterator<'group, 'client, MAX_SLAVES, MAX_PDI, S, DC>
{
    pub(in crate::slave_group) fn new(
        client: &'client Client<'client>,
        group: &'group SlaveGroup<MAX_SLAVES, MAX_PDI, S, DC>,
    ) -> Self {
        Self {
            group,
            idx: 0,
            client,
        }
    }
}

impl<'group, 'client, const MAX_SLAVES: usize, const MAX_PDI: usize, DC> Iterator
    for GroupSlaveIterator<'group, 'client, MAX_SLAVES, MAX_PDI, PreOp, DC>
where
    'client: 'group,
{
    type Item = SlaveRef<'group, AtomicRefMut<'group, Slave>>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.idx >= self.group.len() {
            return None;
        }

        let slave = fmt::unwrap!(self.group.slave(self.client, self.idx).map_err(|e| {
            fmt::error!("Failed to get slave at index {} from group with {} slaves: {}. This is very wrong. Please open an issue.", self.idx, self.group.len(), e);

            e
        }));

        self.idx += 1;

        Some(slave)
    }
}

impl<'group, 'client, const MAX_SLAVES: usize, const MAX_PDI: usize, S, DC> Iterator
    for GroupSlaveIterator<'group, 'client, MAX_SLAVES, MAX_PDI, S, DC>
where
    'client: 'group,
    S: HasPdi,
{
    type Item = SlaveRef<'group, SlavePdi<'group>>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.idx >= self.group.len() {
            return None;
        }

        let slave = fmt::unwrap!(self.group.slave(self.client, self.idx).map_err(|e| {
            fmt::error!("Failed to get slave at index {} from group with {} slaves: {}. This is very wrong. Please open an issue.", self.idx, self.group.len(), e);

            e
        }));

        self.idx += 1;

        Some(slave)
    }
}
