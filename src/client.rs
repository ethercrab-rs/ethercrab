use crate::{
    al_control::AlControl,
    command::Command,
    error::{Error, PduError},
    pdi::{PdiOffset, PdiSegment},
    pdu_loop::{CheckWorkingCounter, PduLoop, PduResponse},
    register::RegisterAddress,
    slave::{Slave, SlaveRef},
    slave_state::SlaveState,
    timer_factory::TimerFactory,
    PduData, PduRead, BASE_SLAVE_ADDR,
};
use core::{
    cell::{RefCell, UnsafeCell},
    marker::PhantomData,
    time::Duration,
};
use packed_struct::PackedStruct;

pub struct Client<
    const MAX_FRAMES: usize,
    const MAX_PDU_DATA: usize,
    const MAX_SLAVES: usize,
    TIMEOUT,
> {
    // TODO: un-pub
    pub pdu_loop: PduLoop<MAX_FRAMES, MAX_PDU_DATA, TIMEOUT>,
    _timeout: PhantomData<TIMEOUT>,
    // TODO: UnsafeCell instead of RefCell?
    slaves: UnsafeCell<heapless::Vec<RefCell<Slave>, MAX_SLAVES>>,
}

unsafe impl<const MAX_FRAMES: usize, const MAX_PDU_DATA: usize, const MAX_SLAVES: usize, TIMEOUT>
    Sync for Client<MAX_FRAMES, MAX_PDU_DATA, MAX_SLAVES, TIMEOUT>
{
}

impl<const MAX_FRAMES: usize, const MAX_PDU_DATA: usize, const MAX_SLAVES: usize, TIMEOUT>
    Client<MAX_FRAMES, MAX_PDU_DATA, MAX_SLAVES, TIMEOUT>
where
    TIMEOUT: TimerFactory,
{
    pub fn new() -> Self {
        // MSRV: Make `MAX_FRAMES` a `u8` when `generic_const_exprs` is stablised
        assert!(
            MAX_FRAMES <= u8::MAX.into(),
            "Packet indexes are u8s, so cache array cannot be any bigger than u8::MAX"
        );

        // MSRV: Make `MAX_SLAVES` a `u16` when `generic_const_exprs` is stablised
        assert!(
            MAX_SLAVES <= u16::MAX.into(),
            "Slave list may only contain up to u16::MAX slaves"
        );

        Self {
            pdu_loop: PduLoop::new(),
            slaves: UnsafeCell::new(heapless::Vec::new()),
            _timeout: PhantomData,
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
    pub async fn init<O>(&self, mut slave_preop_safeop: impl FnMut() -> O) -> Result<(), Error>
    where
        O: core::future::Future<Output = Result<(), Error>>,
    {
        self.reset_slaves().await?;

        // Each slave increments working counter, so we can use it as a total count of slaves
        let (_res, num_slaves) = self.brd::<u8>(RegisterAddress::Type).await?;

        if usize::from(num_slaves) > self.slaves().capacity() {
            return Err(Error::TooManySlaves);
        }

        // Make sure slave list is empty
        self.slaves_mut().truncate(0);

        let mut offset = PdiOffset::default();

        // Set configured address for all discovered slaves
        for slave_idx in 0..num_slaves {
            let address = BASE_SLAVE_ADDR + slave_idx;

            self.apwr(
                slave_idx,
                RegisterAddress::ConfiguredStationAddress,
                address,
            )
            .await?
            .wkc(1, "set station address")?;

            let (new_offset, slave) =
                Slave::configure_from_eeprom(&self, address, offset, &mut slave_preop_safeop)
                    .await?;

            log::debug!(
                "Slave #{:#06x} PDI mapping inputs: {}, outputs: {}",
                address,
                slave.input_range,
                slave.output_range
            );

            offset = new_offset;

            let slave = RefCell::new(slave);

            self.slaves_mut()
                .push(slave)
                // NOTE: This shouldn't fail as we check for capacity above, but it's worth double
                // checking.
                .map_err(|_| Error::TooManySlaves)?;
        }

        log::debug!("Next PDI offset: {:?}", offset);

        self.wait_for_state(SlaveState::SafeOp).await?;

        Ok(())
    }

    pub fn num_slaves(&self) -> usize {
        self.slaves().len()
    }

    fn slaves(&self) -> &heapless::Vec<RefCell<Slave>, MAX_SLAVES> {
        unsafe { &*self.slaves.get() as &heapless::Vec<RefCell<Slave>, MAX_SLAVES> }
    }

    fn slaves_mut(&self) -> &mut heapless::Vec<RefCell<Slave>, MAX_SLAVES> {
        unsafe { &mut *self.slaves.get() as &mut heapless::Vec<RefCell<Slave>, MAX_SLAVES> }
    }

    // DELETEME
    pub fn slave_by_index_pdi_ranges(&self, idx: usize) -> Result<(PdiSegment, PdiSegment), Error> {
        let slave = self
            .slaves()
            .get(idx)
            .ok_or(Error::SlaveNotFound(idx))?
            .try_borrow_mut()
            .map_err(|_| Error::Borrow)?;

        Ok((slave.input_range.clone(), slave.output_range.clone()))
    }

    pub fn slave_by_index(
        &self,
        idx: usize,
    ) -> Result<SlaveRef<'_, MAX_FRAMES, MAX_PDU_DATA, MAX_SLAVES, TIMEOUT>, Error> {
        let slave = self
            .slaves()
            .get(idx)
            .ok_or(Error::SlaveNotFound(idx))?
            .try_borrow_mut()
            .map_err(|_| Error::Borrow)?;

        Ok(SlaveRef::new(self, slave.configured_address))
    }

    /// Request the same state for all slaves.
    pub async fn request_slave_state(&self, desired_state: SlaveState) -> Result<(), Error> {
        let num_slaves = self.slaves().len();

        self.bwr(
            RegisterAddress::AlControl,
            AlControl::new(desired_state).pack().unwrap(),
        )
        .await?
        .wkc(num_slaves as u16, "set all slaves state")?;

        self.wait_for_state(desired_state).await
    }

    pub async fn wait_for_state(&self, desired_state: SlaveState) -> Result<(), Error> {
        let num_slaves = self.slaves().len();

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
        let pdu = self.pdu_loop.pdu_tx(command, &[], T::len()).await?;

        let res = T::try_from_slice(pdu.data()).map_err(|_e| PduError::Decode)?;

        Ok((res, pdu.working_counter()))
    }

    // TODO: Support different I and O types; some things can return different data
    async fn write_service<T>(&self, command: Command, value: T) -> Result<PduResponse<T>, Error>
    where
        T: PduData,
    {
        let pdu = self
            .pdu_loop
            .pdu_tx(command, value.as_slice(), T::len())
            .await?;

        let res = T::try_from_slice(pdu.data()).map_err(|_| PduError::Decode)?;

        Ok((res, pdu.working_counter()))
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
}
