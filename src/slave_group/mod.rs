mod configurator;
mod container;
mod group_slave;

use crate::{
    error::{Error, Item},
    slave::{IoRanges, Slave, SlaveRef},
    timer_factory::TimerFactory,
    Client,
};
use core::{cell::UnsafeCell, future::Future, pin::Pin};
pub use group_slave::GroupSlave;

pub use configurator::Configurator;
pub use container::SlaveGroupContainer;

// TODO: When the right async-trait stuff is stabilised, it should be possible to remove the
// `Box`ing here, and make this work without an allocator. See also
// <https://users.rust-lang.org/t/store-async-closure-on-struct-in-no-std/82929>
type HookFuture<'any> = Pin<Box<dyn Future<Output = Result<(), Error>> + Send + 'any>>;

type HookFn<TIMEOUT> = for<'any> fn(&'any SlaveRef<TIMEOUT>) -> HookFuture<'any>;

/// A group of one or more EtherCAT slaves.
///
/// Groups are created during EtherCrab initialisation, and are the only way to access individual
/// slave PDI sections.
pub struct SlaveGroup<const MAX_SLAVES: usize, const MAX_PDI: usize, TIMEOUT> {
    slaves: heapless::Vec<Slave, MAX_SLAVES>,
    preop_safeop_hook: Option<HookFn<TIMEOUT>>,
    pdi: UnsafeCell<[u8; MAX_PDI]>,
    /// The number of bytes at the beginning of the PDI reserved for slave inputs.
    read_pdi_len: usize,
    /// The total length (I and O) of the PDI for this group.
    pdi_len: usize,
    start_address: u32,
    /// Expected working counter when performing a read/write to all slaves in this group.
    ///
    /// This should be equivalent to `(slaves with inputs) + (2 * slaves with outputs)`.
    group_working_counter: u16,
}

// FIXME: Remove these unsafe impls if possible. There's some weird quirkiness when moving a group
// into an async block going on...
unsafe impl<const MAX_SLAVES: usize, const MAX_PDI: usize, TIMEOUT> Sync
    for SlaveGroup<MAX_SLAVES, MAX_PDI, TIMEOUT>
{
}
unsafe impl<const MAX_SLAVES: usize, const MAX_PDI: usize, TIMEOUT> Send
    for SlaveGroup<MAX_SLAVES, MAX_PDI, TIMEOUT>
{
}

impl<const MAX_SLAVES: usize, const MAX_PDI: usize, TIMEOUT> Default
    for SlaveGroup<MAX_SLAVES, MAX_PDI, TIMEOUT>
{
    fn default() -> Self {
        Self {
            slaves: Default::default(),
            preop_safeop_hook: Default::default(),
            pdi: UnsafeCell::new([0u8; MAX_PDI]),
            read_pdi_len: Default::default(),
            pdi_len: Default::default(),
            start_address: 0,
            group_working_counter: 0,
        }
    }
}

impl<const MAX_SLAVES: usize, const MAX_PDI: usize, TIMEOUT>
    SlaveGroup<MAX_SLAVES, MAX_PDI, TIMEOUT>
{
    /// Create a new slave group with a given PRE OP -> SAFE OP hook.
    ///
    /// The hook can be used to configure slaves using SDOs.
    pub fn new(preop_safeop_hook: HookFn<TIMEOUT>) -> Self {
        Self {
            preop_safeop_hook: Some(preop_safeop_hook),
            ..Default::default()
        }
    }

    /// Add a slave to the group.
    pub fn push(&mut self, slave: Slave) -> Result<(), Error> {
        self.slaves
            .push(slave)
            .map_err(|_| Error::Capacity(Item::Slave))
    }

    /// Get all slaves in this group.
    pub fn slaves(&self) -> &[Slave] {
        &self.slaves
    }

    /// Retrieve a reference to a slave in this group by index.
    pub fn slave(&self, index: usize) -> Result<GroupSlave, Error>
    where
        TIMEOUT: TimerFactory,
    {
        let slave = self.slaves.get(index).ok_or(Error::NotFound {
            item: Item::Slave,
            index: Some(index),
        })?;

        let IoRanges {
            input: input_range,
            output: output_range,
        } = slave.io_segments();

        // SAFETY: Multiple references are ok as long as I and O ranges do not overlap.
        let i_data = self.pdi();
        let o_data = self.pdi_mut();

        let inputs = i_data
            .get(input_range.bytes.clone())
            // TODO: Better error type
            .ok_or(Error::Internal)?;
        let outputs = o_data
            .get(output_range.bytes.clone())
            // TODO: Better error type
            .ok_or(Error::Internal)?;

        Ok(GroupSlave::new(slave, inputs, outputs))
    }

    fn pdi_mut(&self) -> &mut [u8] {
        let all_buf = unsafe { &mut *self.pdi.get() };

        &mut all_buf[0..self.pdi_len]
    }

    fn pdi(&self) -> &[u8] {
        let all_buf = unsafe { &*self.pdi.get() };

        &all_buf[0..self.pdi_len]
    }

    // /// Get the input and output segments of the PDI for a given slave.
    // ///
    // /// If the slave index does not resolve to a discovered slave, this method will return `None`.
    // pub fn io(&self, idx: usize) -> Option<(Option<&[u8]>, Option<&mut [u8]>)> {
    //     let IoRanges {
    //         input: input_range,
    //         output: output_range,
    //     } = self.slaves.get(idx)?.io_segments();

    //     // SAFETY: Multiple references are ok as long as I and O ranges do not overlap.
    //     let i_data = self.pdi();
    //     let o_data = self.pdi_mut();

    //     let i = input_range
    //         .as_ref()
    //         .and_then(|range| i_data.get(range.bytes.clone()));
    //     let o = output_range
    //         .as_ref()
    //         .and_then(|range| o_data.get_mut(range.bytes.clone()));

    //     Some((i, o))
    // }

    /// Drive the slave group's inputs and outputs.
    ///
    /// A `SlaveGroup` will not process any inputs or outputs unless this method is called
    /// periodically. It will send an `LRW` to update slave outputs and read slave inputs.
    pub async fn tx_rx<'client>(
        &self,
        client: &'client Client<'client, TIMEOUT>,
    ) -> Result<(), Error>
    where
        TIMEOUT: TimerFactory,
    {
        let (_res, _wkc) = client
            .lrw_buf(self.start_address, self.pdi_mut(), self.read_pdi_len)
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

    pub(crate) fn as_mut_ref(&mut self) -> Configurator<'_, TIMEOUT> {
        Configurator {
            slaves: self.slaves.as_mut(),
            max_pdi_len: MAX_PDI,
            preop_safeop_hook: self.preop_safeop_hook.as_ref(),
            read_pdi_len: &mut self.read_pdi_len,
            pdi_len: &mut self.pdi_len,
            start_address: &mut self.start_address,
            group_working_counter: &mut self.group_working_counter,
        }
    }
}
