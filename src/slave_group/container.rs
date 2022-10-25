use super::SlaveGroupRef;
use crate::SlaveGroup;

pub trait SlaveGroupContainer<const MAX_FRAMES: usize, const MAX_PDU_DATA: usize, TIMEOUT> {
    fn num_groups(&self) -> usize;

    fn group(&mut self, index: usize) -> Option<SlaveGroupRef<MAX_FRAMES, MAX_PDU_DATA, TIMEOUT>>;

    fn total_slaves(&mut self) -> usize {
        let mut accum = 0;

        for i in 0..self.num_groups() {
            accum += self.group(i).map(|g| g.slaves.len()).unwrap_or(0);
        }

        accum
    }
}

impl<
        const N: usize,
        const MAX_SLAVES: usize,
        const MAX_PDI: usize,
        const MAX_FRAMES: usize,
        const MAX_PDU_DATA: usize,
        TIMEOUT,
    > SlaveGroupContainer<MAX_FRAMES, MAX_PDU_DATA, TIMEOUT>
    for [SlaveGroup<MAX_SLAVES, MAX_PDI, MAX_FRAMES, MAX_PDU_DATA, TIMEOUT>; N]
{
    fn num_groups(&self) -> usize {
        N
    }

    fn group(&mut self, index: usize) -> Option<SlaveGroupRef<MAX_FRAMES, MAX_PDU_DATA, TIMEOUT>> {
        self.get_mut(index).map(|group| group.as_mut_ref())
    }
}

impl<
        const MAX_SLAVES: usize,
        const MAX_PDI: usize,
        const MAX_FRAMES: usize,
        const MAX_PDU_DATA: usize,
        TIMEOUT,
    > SlaveGroupContainer<MAX_FRAMES, MAX_PDU_DATA, TIMEOUT>
    for SlaveGroup<MAX_SLAVES, MAX_PDI, MAX_FRAMES, MAX_PDU_DATA, TIMEOUT>
{
    fn num_groups(&self) -> usize {
        1
    }

    fn group(&mut self, _index: usize) -> Option<SlaveGroupRef<MAX_FRAMES, MAX_PDU_DATA, TIMEOUT>> {
        Some(self.as_mut_ref())
    }
}
