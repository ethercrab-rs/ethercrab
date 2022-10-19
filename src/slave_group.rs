use crate::{
    error::{Error, Item},
    pdi::PdiOffset,
    slave::{
        configurator::SlaveConfigurator, slave_client::SlaveClient, IoRanges, Slave, SlaveRef,
    },
    timer_factory::TimerFactory,
    Client,
};
use core::{cell::UnsafeCell, future::Future, pin::Pin};

// TODO: When the right async-trait stuff is stabilised, it should be possible to remove the
// `Box`ing here, and make this work without an allocator. See also
// <https://users.rust-lang.org/t/store-async-closure-on-struct-in-no-std/82929>
type HookFuture<'any> = Pin<Box<dyn Future<Output = Result<(), Error>> + Send + 'any>>;

type HookFn<TIMEOUT, const MAX_FRAMES: usize, const MAX_PDU_DATA: usize> =
    for<'any> fn(&'any SlaveRef<MAX_FRAMES, MAX_PDU_DATA, TIMEOUT>) -> HookFuture<'any>;

pub trait SlaveGroupContainer<const MAX_FRAMES: usize, const MAX_PDU_DATA: usize, TIMEOUT> {
    fn num_groups(&self) -> usize;

    fn group(&mut self, index: usize) -> Option<SlaveGroupRef<MAX_FRAMES, MAX_PDU_DATA, TIMEOUT>>;
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

pub struct SlaveGroup<
    const MAX_SLAVES: usize,
    const MAX_PDI: usize,
    const MAX_FRAMES: usize,
    const MAX_PDU_DATA: usize,
    TIMEOUT,
> {
    slaves: heapless::Vec<Slave, MAX_SLAVES>,
    preop_safeop_hook: Option<HookFn<TIMEOUT, MAX_FRAMES, MAX_PDU_DATA>>,
    pdi: UnsafeCell<[u8; MAX_PDI]>,
    pdi_len: usize,
    start_address: u32,
    /// Expected working counter when performing a read/write to all slaves in this group.
    ///
    /// This should be equivalent to `(slaves with inputs) + (2 * slaves with outputs)`.
    group_working_counter: u16,
}

// FIXME: Remove these unsafe impls if possible. There's some weird quirkiness when moving a group
// into an async block going on...
unsafe impl<
        const MAX_SLAVES: usize,
        const MAX_PDI: usize,
        const MAX_FRAMES: usize,
        const MAX_PDU_DATA: usize,
        TIMEOUT,
    > Sync for SlaveGroup<MAX_SLAVES, MAX_PDI, MAX_FRAMES, MAX_PDU_DATA, TIMEOUT>
{
}
unsafe impl<
        const MAX_SLAVES: usize,
        const MAX_PDI: usize,
        const MAX_FRAMES: usize,
        const MAX_PDU_DATA: usize,
        TIMEOUT,
    > Send for SlaveGroup<MAX_SLAVES, MAX_PDI, MAX_FRAMES, MAX_PDU_DATA, TIMEOUT>
{
}

impl<
        const MAX_SLAVES: usize,
        const MAX_PDI: usize,
        const MAX_FRAMES: usize,
        const MAX_PDU_DATA: usize,
        TIMEOUT,
    > Default for SlaveGroup<MAX_SLAVES, MAX_PDI, MAX_FRAMES, MAX_PDU_DATA, TIMEOUT>
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
    > SlaveGroup<MAX_SLAVES, MAX_PDI, MAX_FRAMES, MAX_PDU_DATA, TIMEOUT>
{
    pub fn new(preop_safeop_hook: HookFn<TIMEOUT, MAX_FRAMES, MAX_PDU_DATA>) -> Self {
        Self {
            preop_safeop_hook: Some(preop_safeop_hook),
            ..Default::default()
        }
    }

    pub fn push(&mut self, slave: Slave) -> Result<(), Error> {
        self.slaves
            .push(slave)
            .map_err(|_| Error::Capacity(Item::Slave))
    }

    pub fn slaves(&self) -> &[Slave] {
        &self.slaves
    }

    pub fn slave<'a>(
        &'a self,
        index: usize,
        client: &'a Client<'a, MAX_FRAMES, MAX_PDU_DATA, TIMEOUT>,
    ) -> Option<SlaveRef<'a, MAX_FRAMES, MAX_PDU_DATA, TIMEOUT>>
    where
        TIMEOUT: TimerFactory,
    {
        let slave = self.slaves.get(index)?;

        Some(SlaveRef::new(
            SlaveClient::new(client, slave.configured_address),
            slave,
        ))
    }

    fn pdi_mut(&self) -> &mut [u8] {
        unsafe { &mut *self.pdi.get() }
    }

    fn pdi(&self) -> &[u8] {
        unsafe { &*self.pdi.get() }
    }

    /// Get the input and output segments of the PDI for a given slave.
    ///
    /// If the slave index does not resolve to a discovered slave, this method will return `None`.
    pub fn io(&self, idx: usize) -> Option<(Option<&[u8]>, Option<&mut [u8]>)> {
        let IoRanges {
            input: input_range,
            output: output_range,
        } = self.slaves.get(idx)?.io_segments();

        // SAFETY: Multiple references are ok as long as I and O ranges do not overlap.
        let i_data = self.pdi();
        let o_data = self.pdi_mut();

        let i = input_range
            .as_ref()
            .and_then(|range| i_data.get(range.bytes.clone()));
        let o = output_range
            .as_ref()
            .and_then(|range| o_data.get_mut(range.bytes.clone()));

        Some((i, o))
    }

    pub async fn tx_rx<'client>(
        &self,
        client: &'client Client<'client, MAX_FRAMES, MAX_PDU_DATA, TIMEOUT>,
    ) -> Result<(), Error>
    where
        TIMEOUT: TimerFactory,
    {
        let (_res, wkc) = client.lrw_buf(self.start_address, self.pdi_mut()).await?;

        // FIXME: AKD returns 2 when it should be 3. Why?
        // if wkc != self.group_working_counter {
        if usize::from(wkc) < self.slaves.len() {
            Err(Error::WorkingCounter {
                expected: self.slaves.len() as u16,
                received: wkc,
                context: Some("group working counter"),
            })
        } else {
            Ok(())
        }
    }

    pub(crate) fn as_mut_ref(&mut self) -> SlaveGroupRef<'_, MAX_FRAMES, MAX_PDU_DATA, TIMEOUT> {
        SlaveGroupRef {
            slaves: self.slaves.as_mut(),
            max_pdi_len: MAX_PDI,
            preop_safeop_hook: self.preop_safeop_hook.as_ref(),
            pdi_len: &mut self.pdi_len,
            start_address: &mut self.start_address,
            group_working_counter: &mut self.group_working_counter,
        }
    }
}

/// A reference to a [`SlaveGroup`] with erased `MAX_SLAVES` constant.
pub struct SlaveGroupRef<'a, const MAX_FRAMES: usize, const MAX_PDU_DATA: usize, TIMEOUT> {
    pdi_len: &'a mut usize,
    max_pdi_len: usize,
    start_address: &'a mut u32,
    group_working_counter: &'a mut u16,
    slaves: &'a mut [Slave],
    preop_safeop_hook: Option<&'a HookFn<TIMEOUT, MAX_FRAMES, MAX_PDU_DATA>>,
}

impl<'a, const MAX_FRAMES: usize, const MAX_PDU_DATA: usize, TIMEOUT>
    SlaveGroupRef<'a, MAX_FRAMES, MAX_PDU_DATA, TIMEOUT>
where
    TIMEOUT: TimerFactory,
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
            let mut slave_config = SlaveConfigurator::new(client, slave);

            slave_config.configure_mailboxes().await?;

            log::debug!("Slave group configured SAFE-OP");

            if let Some(hook) = self.preop_safeop_hook {
                let conf = slave_config.as_ref();

                let fut = (hook)(&conf);

                fut.await?;
            }

            log::debug!("Slave group configuration hook executed");

            let new_offset = slave_config.configure_fmmus(offset).await?;

            log::debug!("Slave group configured PRE-OP");

            offset = new_offset;

            *self.group_working_counter += slave.config.io.working_counter_sum();
        }

        let pdi_len = (offset.start_address - *self.start_address) as usize;

        if pdi_len > self.max_pdi_len {
            Err(Error::PdiTooLong {
                desired: self.max_pdi_len,
                required: pdi_len,
            })
        } else {
            *self.pdi_len = pdi_len;

            Ok(offset)
        }
    }
}
