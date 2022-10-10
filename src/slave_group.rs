use crate::{
    error::Error,
    pdi::PdiOffset,
    slave::{Slave, SlaveRef},
    timer_factory::TimerFactory,
    Client,
};
use core::cell::UnsafeCell;

type HookFn<TIMEOUT, O, const MAX_FRAMES: usize, const MAX_PDU_DATA: usize> =
    fn(&SlaveRef<MAX_FRAMES, MAX_PDU_DATA, TIMEOUT>) -> O;

pub trait SlaveGroupContainer<'a, const MAX_FRAMES: usize, const MAX_PDU_DATA: usize, TIMEOUT, O> {
    fn num_groups(&self) -> usize;

    fn group(
        &'a mut self,
        index: usize,
    ) -> Option<SlaveGroupRef<'a, MAX_FRAMES, MAX_PDU_DATA, TIMEOUT, O>>;
}

impl<
        'a,
        const N: usize,
        const MAX_SLAVES: usize,
        const MAX_PDI: usize,
        const MAX_FRAMES: usize,
        const MAX_PDU_DATA: usize,
        TIMEOUT,
        O,
    > SlaveGroupContainer<'a, MAX_FRAMES, MAX_PDU_DATA, TIMEOUT, O>
    for [SlaveGroup<MAX_SLAVES, MAX_PDI, MAX_FRAMES, MAX_PDU_DATA, TIMEOUT, O>; N]
{
    fn num_groups(&self) -> usize {
        N
    }

    fn group(
        &'a mut self,
        index: usize,
    ) -> Option<SlaveGroupRef<'a, MAX_FRAMES, MAX_PDU_DATA, TIMEOUT, O>> {
        self.get_mut(index).map(|group| group.as_mut_ref())
    }
}

pub struct SlaveGroup<
    const MAX_SLAVES: usize,
    const MAX_PDI: usize,
    const MAX_FRAMES: usize,
    const MAX_PDU_DATA: usize,
    TIMEOUT,
    O,
> {
    slaves: heapless::Vec<Slave, MAX_SLAVES>,
    preop_safeop_hook: Option<HookFn<TIMEOUT, O, MAX_FRAMES, MAX_PDU_DATA>>,
    pdi: UnsafeCell<[u8; MAX_PDI]>,
    pdi_len: usize,
    start_address: u32,
    /// Expected working counter when performing a read/write to all slaves in this group.
    ///
    /// This should be equivalent to `(slaves with inputs) + (2 * slaves with outputs)`.
    group_working_counter: u16,
}

impl<
        const MAX_SLAVES: usize,
        const MAX_PDI: usize,
        const MAX_FRAMES: usize,
        const MAX_PDU_DATA: usize,
        TIMEOUT,
        O,
    > Default for SlaveGroup<MAX_SLAVES, MAX_PDI, MAX_FRAMES, MAX_PDU_DATA, TIMEOUT, O>
{
    fn default() -> Self {
        Self {
            slaves: Default::default(),
            preop_safeop_hook: Default::default(),
            pdi: UnsafeCell::new([0u8; MAX_PDI]),
            pdi_len: Default::default(),
            start_address: 0,
            group_working_counter: 0,
        }
    }
}

impl<
        const MAX_SLAVES: usize,
        const MAX_PDI: usize,
        const MAX_FRAMES: usize,
        const MAX_PDU_DATA: usize,
        TIMEOUT,
        O,
    > SlaveGroup<MAX_SLAVES, MAX_PDI, MAX_FRAMES, MAX_PDU_DATA, TIMEOUT, O>
{
    pub fn new(preop_safeop_hook: fn(&SlaveRef<MAX_FRAMES, MAX_PDU_DATA, TIMEOUT>) -> O) -> Self {
        Self {
            preop_safeop_hook: Some(preop_safeop_hook),
            ..Default::default()
        }
    }

    pub fn push(&mut self, slave: Slave) -> Result<(), Error> {
        self.slaves.push(slave).map_err(|_| Error::TooManySlaves)
    }

    pub fn slaves(&self) -> &[Slave] {
        &self.slaves
    }

    fn pdi(&self) -> &mut [u8] {
        unsafe { &mut *self.pdi.get() }
    }

    pub fn io(&self, idx: usize) -> Option<(Option<&mut [u8]>, Option<&mut [u8]>)> {
        let (input_range, output_range) = self.slaves.get(idx)?.io_segments();

        // SAFETY: Multiple mutable references are ok as long as I and O ranges do not overlap.
        let data = self.pdi();
        let data2 = self.pdi();

        let i = input_range
            .clone()
            .and_then(|range| data.get_mut(range.bytes.clone()));
        let o = output_range
            .clone()
            .and_then(|range| data2.get_mut(range.bytes.clone()));

        Some((i, o))
    }

    pub async fn tx_rx<'client>(
        &mut self,
        client: &'client Client<'client, MAX_FRAMES, MAX_PDU_DATA, TIMEOUT>,
    ) -> Result<(), Error>
    where
        TIMEOUT: TimerFactory,
    {
        let (_res, _wkc) = client.lrw_buf(self.start_address, self.pdi()).await?;

        // TODO: Check working counter = (slaves with outputs) + (slaves with inputs * 2)

        if _wkc != self.group_working_counter {
            return Err(Error::WorkingCounter {
                expected: self.group_working_counter,
                received: _wkc,
                context: Some("group working counter"),
            });
        } else {
            Ok(())
        }
    }

    // TODO: AsRef or AsMut trait?
    pub(crate) fn as_mut_ref(&mut self) -> SlaveGroupRef<'_, MAX_FRAMES, MAX_PDU_DATA, TIMEOUT, O> {
        SlaveGroupRef {
            slaves: self.slaves.as_mut(),
            preop_safeop_hook: self.preop_safeop_hook,
            pdi_len: &mut self.pdi_len,
            start_address: &mut self.start_address,
            group_working_counter: &mut self.group_working_counter,
        }
    }
}

/// A reference to a [`SlaveGroup`] with elided `MAX_SLAVES` constant.
pub struct SlaveGroupRef<'a, const MAX_FRAMES: usize, const MAX_PDU_DATA: usize, TIMEOUT, O> {
    pdi_len: &'a mut usize,
    start_address: &'a mut u32,
    group_working_counter: &'a mut u16,
    slaves: &'a mut [Slave],
    preop_safeop_hook: Option<HookFn<TIMEOUT, O, MAX_FRAMES, MAX_PDU_DATA>>,
}

impl<'a, const MAX_FRAMES: usize, const MAX_PDU_DATA: usize, TIMEOUT, O>
    SlaveGroupRef<'a, MAX_FRAMES, MAX_PDU_DATA, TIMEOUT, O>
where
    TIMEOUT: TimerFactory,
    O: core::future::Future<Output = ()>,
{
    pub(crate) async fn configure_from_eeprom<'client>(
        &mut self,
        mut offset: PdiOffset,
        client: &'client Client<'client, MAX_FRAMES, MAX_PDU_DATA, TIMEOUT>,
    ) -> Result<PdiOffset, Error>
    where
        TIMEOUT: TimerFactory,
    {
        *self.start_address = offset.start_address;

        for slave in self.slaves.iter_mut() {
            let mut slave_ref = SlaveRef::new(client, slave.configured_address);

            slave_ref.configure_from_eeprom_safe_op().await?;

            if let Some(hook) = self.preop_safeop_hook {
                (hook)(&slave_ref);
            }

            let (new_offset, i, o) = slave_ref.configure_from_eeprom_pre_op(offset).await?;

            slave.input_range = i.clone();
            slave.output_range = o.clone();

            offset = new_offset;

            *self.group_working_counter += i.map(|_| 1).unwrap_or(0) + o.map(|_| 2).unwrap_or(0);
        }

        *self.pdi_len = (offset.start_address - *self.start_address) as usize;

        Ok(offset)
    }
}
