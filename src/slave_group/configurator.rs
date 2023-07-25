use atomic_refcell::AtomicRefCell;

use crate::{
    error::Error,
    pdi::PdiOffset,
    register::RegisterAddress,
    slave::{configuration::PdoDirection, Slave, SlaveRef},
    Client, SlaveGroup,
};
use core::time::Duration;

#[derive(Debug)]
struct GroupInnerRef<'a> {
    slaves: &'a mut [AtomicRefCell<Slave>],
    /// The number of bytes at the beginning of the PDI reserved for slave inputs.
    read_pdi_len: &'a mut usize,
    /// The total length (I and O) of the PDI for this group.
    pdi_len: &'a mut usize,
    pdi_start: &'a mut PdiOffset,
}

// TODO: Prove if this is safe. All this stuff is internal to the crate and short lived so I think
// we might be ok... lol probably not :(.
//
// This is in response to <https://github.com/ethercrab-rs/ethercrab/issues/56#issuecomment-1618904531>
unsafe impl<'a> Sync for SlaveGroupRef<'a> {}
unsafe impl<'a> Send for SlaveGroupRef<'a> {}

/// A reference to a [`SlaveGroup`](crate::SlaveGroup) returned by the closure passed to
/// [`Client::init`](crate::Client::init).
pub struct SlaveGroupRef<'a> {
    max_pdi_len: usize,
    inner: GroupInnerRef<'a>,
}

impl<'a> SlaveGroupRef<'a> {
    pub(in crate::slave_group) fn new<const MAX_SLAVES: usize, const MAX_PDI: usize, S>(
        group: &'a SlaveGroup<MAX_SLAVES, MAX_PDI, S>,
    ) -> Self {
        Self {
            max_pdi_len: MAX_PDI,
            inner: {
                let inner = unsafe { group.inner.get().as_mut().unwrap() };

                GroupInnerRef {
                    slaves: &mut inner.slaves,
                    read_pdi_len: &mut inner.read_pdi_len,
                    pdi_len: &mut inner.pdi_len,
                    pdi_start: &mut inner.pdi_start,
                }
            },
        }
    }

    /// Initialise all slaves in the group and place them in PRE-OP.
    pub(crate) async fn into_pre_op<'sto>(
        &mut self,
        pdi_position: PdiOffset,
        client: &'sto Client<'sto>,
    ) -> Result<PdiOffset, Error> {
        let inner = &mut self.inner;

        // Set the starting position in the PDI for this group's segment
        *inner.pdi_start = pdi_position;

        log::debug!(
            "Going to configure group with {} slave(s), starting PDI offset {:#08x}",
            inner.slaves.len(),
            inner.pdi_start.start_address
        );

        // Configure master read PDI mappings in the first section of the PDI
        for slave in inner.slaves.iter_mut() {
            let slave = slave.get_mut();

            let mut slave_config = SlaveRef::new(client, slave.configured_address, slave);

            // TODO: Move PRE-OP transition out of this so we can do it for the group just once
            slave_config.configure_mailboxes().await?;
        }

        Ok(pdi_position.increment(self.max_pdi_len as u16))
    }
}
