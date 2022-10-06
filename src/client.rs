use crate::{
    al_control::AlControl,
    command::Command,
    error::{Error, PduError},
    pdi::PdiOffset,
    pdu_loop::{CheckWorkingCounter, PduLoop, PduLoopRef, PduResponse},
    register::RegisterAddress,
    slave::Slave,
    slave_group::{SlaveGroup, SlaveGroupRef},
    slave_state::SlaveState,
    timer_factory::TimerFactory,
    PduData, PduRead, BASE_SLAVE_ADDR,
};
use core::{
    cell::{Ref, RefCell},
    fmt::Display,
    marker::PhantomData,
    ops::IndexMut,
    time::Duration,
};
use packed_struct::PackedStruct;

pub struct Client<const MAX_FRAMES: usize, const MAX_PDU_DATA: usize, TIMEOUT> {
    // TODO: un-pub
    pub pdu_loop: PduLoop<MAX_FRAMES, MAX_PDU_DATA, TIMEOUT>,
    num_slaves: RefCell<u16>,
    _timeout: PhantomData<TIMEOUT>,
    // slaves: UnsafeCell<heapless::Vec<RefCell<Slave>, MAX_SLAVES>>,
}

pub struct ClientRef<'a> {
    pdu_loop: PduLoopRef<'a>,
    // num_slaves: u16,
}

impl<'a> ClientRef<'a> {
    // TODO: Dedupe with write_service when refactoring allows
    async fn read_service<T>(&self, command: Command) -> Result<PduResponse<T>, Error>
    where
        T: PduRead,
    {
        let (data, working_counter) = self.pdu_loop.pdu_tx(command, &[], T::len()).await?;

        let res = T::try_from_slice(&data).map_err(|_e| {
            log::error!("PDU data decode");

            PduError::Decode
        })?;

        Ok((res, working_counter))
    }

    // TODO: Support different I and O types; some things can return different data
    async fn write_service<T>(&self, command: Command, value: T) -> Result<PduResponse<T>, Error>
    where
        T: PduData,
    {
        let (data, working_counter) = self
            .pdu_loop
            .pdu_tx(command, value.as_slice(), T::len())
            .await?;

        let res = T::try_from_slice(&data).map_err(|_| PduError::Decode)?;

        Ok((res, working_counter))
    }

    /// Configured address read.
    pub async fn fprd<T>(
        &self,
        address: u16,
        register: RegisterAddress,
    ) -> Result<PduResponse<T>, Error>
    where
        T: PduRead,
    {
        self.read_service(Command::Fprd {
            address,
            register: register.into(),
        })
        .await
    }

    /// Configured address write.
    pub async fn fpwr<T>(
        &self,
        address: u16,
        register: RegisterAddress,
        value: T,
    ) -> Result<PduResponse<T>, Error>
    where
        T: PduData,
    {
        self.write_service(
            Command::Fpwr {
                address,
                register: register.into(),
            },
            value,
        )
        .await
    }

    pub(crate) fn another_one(&self) -> Self {
        ClientRef {
            pdu_loop: self.pdu_loop,
        }
    }
}

unsafe impl<const MAX_FRAMES: usize, const MAX_PDU_DATA: usize, TIMEOUT> Sync
    for Client<MAX_FRAMES, MAX_PDU_DATA, TIMEOUT>
{
}

pub trait SlaveGroupContainer<'a> {
    fn num_groups(&self) -> usize;

    fn group(&'a mut self, index: usize) -> Option<SlaveGroupRef<'a>>;
}

impl<'a, const MAX_SLAVES: usize, const N: usize> SlaveGroupContainer<'a>
    for [SlaveGroup<MAX_SLAVES>; N]
{
    fn num_groups(&self) -> usize {
        N
    }

    fn group(&'a mut self, index: usize) -> Option<SlaveGroupRef<'a>> {
        self.get_mut(index).map(|group| group.as_mut_ref())
    }
}

impl<const MAX_FRAMES: usize, const MAX_PDU_DATA: usize, TIMEOUT>
    Client<MAX_FRAMES, MAX_PDU_DATA, TIMEOUT>
where
    TIMEOUT: TimerFactory,
{
    pub fn new() -> Self {
        // MSRV: Make `MAX_FRAMES` a `u8` when `generic_const_exprs` is stablised
        assert!(
            MAX_FRAMES <= u8::MAX.into(),
            "Packet indexes are u8s, so cache array cannot be any bigger than u8::MAX"
        );

        Self {
            pdu_loop: PduLoop::new(),
            // slaves: UnsafeCell::new(heapless::Vec::new()),
            // TODO: Make the RefCell go away somehow
            num_slaves: RefCell::new(0),
            _timeout: PhantomData,
        }
    }

    pub fn as_ref<'a>(&'a self) -> ClientRef<'a> {
        ClientRef {
            pdu_loop: self.pdu_loop.as_ref(),
            // num_slaves: *self.num_slaves.borrow(),
        }
    }

    /// Write zeroes to every slave's memory in chunks of [`MAX_PDU_DATA`].
    async fn blank_memory(&self, start: impl Into<u16>, len: u16) -> Result<(), Error> {
        let start: u16 = start.into();
        let step = MAX_PDU_DATA;
        let range = start..(start + len);

        for chunk_start in range.step_by(step) {
            self.write_service(
                Command::Bwr {
                    address: 0,
                    register: chunk_start,
                },
                [0u8; MAX_PDU_DATA],
            )
            .await?;
        }

        Ok(())
    }

    async fn reset_slaves(&self) -> Result<(), Error> {
        // Reset slaves to init
        self.bwr(
            RegisterAddress::AlControl,
            AlControl::reset().pack().unwrap(),
        )
        .await?;

        // Clear FMMUs. FMMU memory section is 0xff (255) bytes long - see ETG1000.4 Table 57
        self.blank_memory(RegisterAddress::Fmmu0, 0xff).await?;

        // Clear SMs. SM memory section is 0x7f bytes long - see ETG1000.4 Table 59
        self.blank_memory(RegisterAddress::Sm0, 0x7f).await?;

        Ok(())
    }

    /// Detect slaves and set their configured station addresses.
    pub async fn init<G>(
        &self,
        mut groups: G,
        mut group_filter: impl FnMut(&mut G, Slave),
    ) -> Result<G, Error>
    where
        G: for<'a> SlaveGroupContainer<'a>,
    {
        self.reset_slaves().await?;

        // Each slave increments working counter, so we can use it as a total count of slaves
        let (_res, num_slaves) = self.brd::<u8>(RegisterAddress::Type).await?;

        // if usize::from(num_slaves) > MAX_SLAVES {
        //     return Err(Error::TooManySlaves);
        // }

        *self.num_slaves.borrow_mut() = num_slaves;

        // NOTE: .map() because SlaveGroup is not copy
        // let mut groups = [0u8; MAX_GROUPS].map(|_| SlaveGroup::default());
        // let mut group_offsets = [PdiOffset::default(); MAX_GROUPS];

        // Set configured address for all discovered slaves
        for slave_idx in 0..num_slaves {
            let configured_address = BASE_SLAVE_ADDR + slave_idx;

            self.apwr(
                slave_idx,
                RegisterAddress::ConfiguredStationAddress,
                configured_address,
            )
            .await?
            .wkc(1, "set station address")?;

            // let (new_offset, slave) =
            //     Slave::configure_from_eeprom(&self, address, offset, &mut || async {
            //         // TODO: Store PO2SO hook on slave. Currently blocked by `Client` having so many const generics
            //         Ok(())
            //     })
            //     .await?;

            // TODO: Instead of just a configured address, read some basic slave data into the
            // struct so the user has more to work with in the grouping function.
            let slave = Slave::new(configured_address);

            // log::debug!(
            //     "Slave #{:#06x} PDI mapping inputs: {}, outputs: {}",
            //     address,
            //     slave.input_range,
            //     slave.output_range
            // );

            // offset = new_offset;

            group_filter(&mut groups, slave);

            // let slave = RefCell::new(slave);

            // groups[0]
            //     .slaves
            //     .push(slave)
            //     // NOTE: This shouldn't fail as we check for capacity above, but it's worth double
            //     // checking.
            //     .map_err(|_| Error::TooManySlaves)?;
        }

        let mut offset = PdiOffset::default();

        // Loop through groups and configure the slaves in each one.
        for i in 0..groups.num_groups() {
            // TODO: Better error type for broken group index calculation
            let mut group = groups.group(i).ok_or_else(|| Error::Other)?;

            offset = group.configure_from_eeprom(offset, self.as_ref()).await?;

            log::debug!("After group #{i} offset: {:?}", offset);
        }

        self.wait_for_state(SlaveState::SafeOp).await?;

        Ok(groups)
    }

    // pub fn num_slaves(&self) -> usize {
    //     usize::from(*self.num_slaves.borrow())
    // }

    // fn slaves(&self) -> &heapless::Vec<RefCell<Slave>, MAX_SLAVES> {
    //     unsafe { &*self.slaves.get() as &heapless::Vec<RefCell<Slave>, MAX_SLAVES> }
    // }

    // fn slaves_mut(&self) -> &mut heapless::Vec<RefCell<Slave>, MAX_SLAVES> {
    //     unsafe { &mut *self.slaves.get() as &mut heapless::Vec<RefCell<Slave>, MAX_SLAVES> }
    // }

    // // DELETEME
    // pub fn slave_by_index_pdi_ranges(&self, idx: usize) -> Result<(PdiSegment, PdiSegment), Error> {
    //     let slave = self
    //         .slaves()
    //         .get(idx)
    //         .ok_or(Error::SlaveNotFound(idx))?
    //         .try_borrow_mut()
    //         .map_err(|_| Error::Borrow)?;

    //     Ok((slave.input_range.clone(), slave.output_range.clone()))
    // }

    // pub fn slave_by_index(
    //     &self,
    //     idx: usize,
    // ) -> Result<SlaveRef<'_, MAX_FRAMES, MAX_PDU_DATA, MAX_SLAVES, TIMEOUT>, Error> {
    //     let slave = self
    //         .slaves()
    //         .get(idx)
    //         .ok_or(Error::SlaveNotFound(idx))?
    //         .try_borrow_mut()
    //         .map_err(|_| Error::Borrow)?;

    //     Ok(SlaveRef::new(self, slave.configured_address))
    // }

    /// Request the same state for all slaves.
    pub async fn request_slave_state(&self, desired_state: SlaveState) -> Result<(), Error> {
        let num_slaves = *self.num_slaves.borrow();

        self.bwr(
            RegisterAddress::AlControl,
            AlControl::new(desired_state).pack().unwrap(),
        )
        .await?
        .wkc(num_slaves as u16, "set all slaves state")?;

        self.wait_for_state(desired_state).await
    }

    pub async fn wait_for_state(&self, desired_state: SlaveState) -> Result<(), Error> {
        let num_slaves = *self.num_slaves.borrow();

        // TODO: Configurable timeout depending on current -> next states
        crate::timeout::<TIMEOUT, _, _>(Duration::from_millis(5000), async {
            loop {
                let status = self
                    .brd::<AlControl>(RegisterAddress::AlStatus)
                    .await?
                    .wkc(num_slaves as u16, "read all slaves state")?;
                if status.state == desired_state {
                    break Result::<(), Error>::Ok(());
                }

                TIMEOUT::timer(Duration::from_millis(10)).await;
            }
        })
        .await
    }

    // TODO: Dedupe with write_service when refactoring allows
    async fn read_service<T>(&self, command: Command) -> Result<PduResponse<T>, Error>
    where
        T: PduRead,
    {
        let pdu_loop = self.pdu_loop.as_ref();

        let (data, working_counter) = pdu_loop.pdu_tx(command, &[], T::len()).await?;

        let res = T::try_from_slice(&data).map_err(|_e| {
            log::error!("PDU data decode");

            PduError::Decode
        })?;

        Ok((res, working_counter))
    }

    // TODO: Support different I and O types; some things can return different data
    async fn write_service<T>(&self, command: Command, value: T) -> Result<PduResponse<T>, Error>
    where
        T: PduData,
    {
        let pdu_loop = self.pdu_loop.as_ref();

        let (data, working_counter) = pdu_loop.pdu_tx(command, value.as_slice(), T::len()).await?;

        let res = T::try_from_slice(&data).map_err(|_| PduError::Decode)?;

        Ok((res, working_counter))
    }

    pub async fn brd<T>(&self, register: RegisterAddress) -> Result<PduResponse<T>, Error>
    where
        T: PduRead,
    {
        self.read_service(Command::Brd {
            // Address is always zero when sent from master
            address: 0,
            register: register.into(),
        })
        .await
    }

    /// Broadcast write.
    pub async fn bwr<T>(&self, register: RegisterAddress, value: T) -> Result<PduResponse<T>, Error>
    where
        T: PduData,
    {
        self.write_service(
            Command::Bwr {
                address: 0,
                register: register.into(),
            },
            value,
        )
        .await
    }

    /// Auto Increment Physical Read.
    pub async fn aprd<T>(
        &self,
        address: u16,
        register: RegisterAddress,
    ) -> Result<PduResponse<T>, Error>
    where
        T: PduRead,
    {
        self.read_service(Command::Aprd {
            address: 0u16.wrapping_sub(address),
            register: register.into(),
        })
        .await
    }

    /// Auto Increment Physical Write.
    pub async fn apwr<T>(
        &self,
        address: u16,
        register: RegisterAddress,
        value: T,
    ) -> Result<PduResponse<T>, Error>
    where
        T: PduData,
    {
        self.write_service(
            Command::Apwr {
                address: 0u16.wrapping_sub(address),
                register: register.into(),
            },
            value,
        )
        .await
    }

    // /// Configured address read.
    // pub async fn fprd<T>(
    //     &self,
    //     address: u16,
    //     register: RegisterAddress,
    // ) -> Result<PduResponse<T>, Error>
    // where
    //     T: PduRead,
    // {
    //     self.read_service(Command::Fprd {
    //         address,
    //         register: register.into(),
    //     })
    //     .await
    // }

    // /// Configured address write.
    // pub async fn fpwr<T>(
    //     &self,
    //     address: u16,
    //     register: RegisterAddress,
    //     value: T,
    // ) -> Result<PduResponse<T>, Error>
    // where
    //     T: PduData,
    // {
    //     self.write_service(
    //         Command::Fpwr {
    //             address,
    //             register: register.into(),
    //         },
    //         value,
    //     )
    //     .await
    // }

    /// Logical write.
    pub async fn lwr<T>(&self, address: u32, value: T) -> Result<PduResponse<T>, Error>
    where
        T: PduData,
    {
        self.write_service(Command::Lwr { address }, value).await
    }

    /// Logical read/write.
    pub async fn lrw<T>(&self, address: u32, value: T) -> Result<PduResponse<T>, Error>
    where
        T: PduData,
    {
        self.write_service(Command::Lrw { address }, value).await
    }

    /// Logical read/write, but direct from/to a mutable slice.
    // TODO: Chunked sends if buffer is too long for MAX_PDU_DATA
    pub async fn lrw_buf<'buf>(
        &self,
        address: u32,
        value: &'buf mut [u8],
    ) -> Result<PduResponse<&'buf mut [u8]>, Error> {
        let pdu_loop = self.pdu_loop.as_ref();

        let (data, working_counter) = pdu_loop
            .pdu_tx(Command::Lrw { address }, value, value.len() as u16)
            .await?;

        if data.len() != value.len() {
            return Err(Error::Pdu(PduError::Decode));
        }

        value.copy_from_slice(&data);

        Ok((value, working_counter))
    }
}
