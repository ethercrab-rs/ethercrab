use crate::{
    al_control::AlControl,
    al_status::AlState,
    command::Command,
    error::{Error, PduError},
    fmmu::Fmmu,
    pdu::{CheckWorkingCounter, PduResponse},
    pdu_loop::PduLoop,
    register::RegisterAddress,
    slave::{Slave, SlaveRef},
    sync_manager_channel::SyncManagerChannel,
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

    /// Detect slaves and set their configured station addresses.
    // TODO: Ability to pass in configs read from ESI files
    pub async fn init(&self) -> Result<(), Error> {
        // Reset everything
        {
            // Reset slaves to init
            self.bwr(
                RegisterAddress::AlControl,
                AlControl::reset().pack().unwrap(),
            )
            .await?;

            // Clear SMs
            // TODO: Read EEPROM and iterate through clearing as many items as detected
            {
                self.bwr(
                    RegisterAddress::Sm0,
                    SyncManagerChannel::default().pack().unwrap(),
                )
                .await?;
                self.bwr(
                    RegisterAddress::Sm1,
                    SyncManagerChannel::default().pack().unwrap(),
                )
                .await?;
                self.bwr(
                    RegisterAddress::Sm2,
                    SyncManagerChannel::default().pack().unwrap(),
                )
                .await?;
                self.bwr(
                    RegisterAddress::Sm3,
                    SyncManagerChannel::default().pack().unwrap(),
                )
                .await?;
            }

            // Clear FMMUs
            // TODO: Read EEPROM and iterate through clearing as many items as detected
            {
                self.bwr(RegisterAddress::Fmmu0, Fmmu::default().pack().unwrap())
                    .await?;
                self.bwr(RegisterAddress::Fmmu1, Fmmu::default().pack().unwrap())
                    .await?;
                self.bwr(RegisterAddress::Fmmu2, Fmmu::default().pack().unwrap())
                    .await?;
                self.bwr(RegisterAddress::Fmmu3, Fmmu::default().pack().unwrap())
                    .await?;
            }

            // TODO: Store EEPROM read size (4 or 8 bytes) in slave on init - this is done by
            // reading the EEPROM status and checking the read size flag
        }

        // Each slave increments working counter, so we can use it as a total count of slaves
        let (_res, num_slaves) = self.brd::<u8>(RegisterAddress::Type).await?;

        if usize::from(num_slaves) > self.slaves().capacity() {
            return Err(Error::TooManySlaves);
        }

        // Make sure slave list is empty
        self.slaves_mut().truncate(0);

        for slave_idx in 0..num_slaves {
            let address = BASE_SLAVE_ADDR + slave_idx;

            self.apwr(
                slave_idx,
                RegisterAddress::ConfiguredStationAddress,
                address,
            )
            .await?
            .wkc(1, "set station address")?;

            let slave_state = self
                .fprd::<AlControl>(address, RegisterAddress::AlStatus)
                .await?
                .wkc(1, "get AL status")?;

            self.slaves_mut()
                .push(RefCell::new(Slave::new(address, slave_state.state)))
                // NOTE: This shouldn't fail as we check for capacity above, but it's worth double
                // checking.
                .map_err(|_| Error::TooManySlaves)?;
        }

        Ok(())
    }

    fn slaves(&self) -> &heapless::Vec<RefCell<Slave>, MAX_SLAVES> {
        unsafe { &*self.slaves.get() as &heapless::Vec<RefCell<Slave>, MAX_SLAVES> }
    }

    fn slaves_mut(&self) -> &mut heapless::Vec<RefCell<Slave>, MAX_SLAVES> {
        unsafe { &mut *self.slaves.get() as &mut heapless::Vec<RefCell<Slave>, MAX_SLAVES> }
    }

    pub fn slave_by_index(
        &self,
        idx: u16,
    ) -> Result<SlaveRef<'_, MAX_FRAMES, MAX_PDU_DATA, MAX_SLAVES, TIMEOUT>, Error> {
        let idx = usize::from(idx);

        let slave = self
            .slaves()
            .get(idx)
            .ok_or(Error::SlaveNotFound(idx))?
            .try_borrow_mut()
            .map_err(|_| Error::Borrow)?;

        Ok(SlaveRef::new(self, slave))
    }

    pub async fn request_slave_state(&self, desired_state: AlState) -> Result<(), Error> {
        let num_slaves = self.slaves().len();

        self.bwr(
            RegisterAddress::AlControl,
            AlControl::new(desired_state).pack().unwrap(),
        )
        .await?
        .wkc(num_slaves as u16, "set all slaves state")?;

        // TODO: Configurable timeout depending on current -> next states
        crate::timeout::<TIMEOUT, _, _>(Duration::from_millis(1000), async {
            loop {
                let control = self
                    .brd::<AlControl>(RegisterAddress::AlControl)
                    .await?
                    .wkc(num_slaves as u16, "read all slaves state")?;

                if control.state == desired_state {
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
        <T as PduRead>::Error: core::fmt::Debug,
    {
        let pdu = self.pdu_loop.pdu_tx(command, &[], T::len()).await?;

        let res = T::try_from_slice(pdu.data()).map_err(|_e| PduError::Decode)?;

        Ok((res, pdu.working_counter()))
    }

    // TODO: Support different I and O types; some things can return different data
    async fn write_service<T>(&self, command: Command, value: T) -> Result<PduResponse<T>, Error>
    where
        T: PduData,
        <T as PduRead>::Error: core::fmt::Debug,
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
        <T as PduRead>::Error: core::fmt::Debug,
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
        <T as PduRead>::Error: core::fmt::Debug,
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
        <T as PduRead>::Error: core::fmt::Debug,
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
        <T as PduRead>::Error: core::fmt::Debug,
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
        <T as PduRead>::Error: core::fmt::Debug,
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
        <T as PduRead>::Error: core::fmt::Debug,
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
        <T as PduRead>::Error: core::fmt::Debug,
    {
        self.write_service(Command::Lwr { address }, value).await
    }
}
