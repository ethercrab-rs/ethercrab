use crate::{Client, SlaveGroupState, SlaveRef};

/// An iterator over all slaves in a group.
///
/// Created by calling [`SlaveGroup::iter`](crate::slave_group::SlaveGroup::iter).
pub struct GroupSlaveIterator<'group, 'client, const MAX_SLAVES: usize, const MAX_PDI: usize, S> {
    group: &'group S,
    idx: usize,
    client: &'client Client<'client>,
}

impl<'group, 'client, const MAX_SLAVES: usize, const MAX_PDI: usize, S>
    GroupSlaveIterator<'group, 'client, MAX_SLAVES, MAX_PDI, S>
{
    pub(in crate::slave_group) fn new(client: &'client Client<'client>, group: &'group S) -> Self {
        Self {
            group,
            idx: 0,
            client,
        }
    }
}

impl<'group, 'client, const MAX_SLAVES: usize, const MAX_PDI: usize, S> Iterator
    for GroupSlaveIterator<'group, 'client, MAX_SLAVES, MAX_PDI, S>
where
    'client: 'group,
    S: SlaveGroupState,
{
    type Item = SlaveRef<'group, S::RefType<'group>>;

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
