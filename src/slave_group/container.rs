use super::SlaveGroupRef;
use crate::SlaveGroup;

/// This trait must be implemented for the item passed to
/// [`Client::init`](crate::client::Client::init).
///
/// For convenience, this trait is already implemented for [`SlaveGroup`] for single-group use
/// cases, as well as `[SlaveGroup; N]` for simple, multi-group uses.
pub trait SlaveGroupContainer<TIMEOUT> {
    /// The number of slave groups in the container.
    fn num_groups(&self) -> usize;

    /// Get a group by index.
    fn group(&mut self, index: usize) -> Option<SlaveGroupRef<TIMEOUT>>;

    /// Count the total number of slave devices held across all groups in this container.
    fn total_slaves(&mut self) -> usize {
        let mut accum = 0;

        for i in 0..self.num_groups() {
            accum += self.group(i).map(|g| g.slaves.len()).unwrap_or(0);
        }

        accum
    }
}

impl<const N: usize, const MAX_SLAVES: usize, const MAX_PDI: usize, TIMEOUT>
    SlaveGroupContainer<TIMEOUT> for [SlaveGroup<MAX_SLAVES, MAX_PDI, TIMEOUT>; N]
{
    fn num_groups(&self) -> usize {
        N
    }

    fn group(&mut self, index: usize) -> Option<SlaveGroupRef<TIMEOUT>> {
        self.get_mut(index).map(|group| group.as_mut_ref())
    }
}

impl<const MAX_SLAVES: usize, const MAX_PDI: usize, TIMEOUT> SlaveGroupContainer<TIMEOUT>
    for SlaveGroup<MAX_SLAVES, MAX_PDI, TIMEOUT>
{
    fn num_groups(&self) -> usize {
        1
    }

    fn group(&mut self, _index: usize) -> Option<SlaveGroupRef<TIMEOUT>> {
        Some(self.as_mut_ref())
    }
}
