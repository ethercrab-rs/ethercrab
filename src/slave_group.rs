use crate::slave::Slave;

// TODO: Can probably dedupe with pdi::Pdi?
#[derive(Debug, Default)]
pub struct SlaveGroup<const MAX_SLAVES: usize> {
    // TODO: Un-pub
    pub slaves: heapless::Vec<Slave, MAX_SLAVES>,
}
