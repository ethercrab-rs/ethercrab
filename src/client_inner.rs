use crate::{
    al_control::AlControl,
    al_status::AlState,
    al_status_code::AlStatusCode,
    command::Command,
    error::{Error, PduError},
    pdu::{CheckWorkingCounter, PduResponse},
    pdu_loop::PduLoop,
    register::RegisterAddress,
    sii::{SiiControl, SiiRequest},
    slave::Slave,
    timer_factory::TimerFactory,
    PduData, PduRead, BASE_SLAVE_ADDR,
};
use core::{cell::RefCell, marker::PhantomData};
use futures::future::{select, Either};
use packed_struct::PackedStruct;

// TODO: Use atomic_refcell crate
// TODO: Move core PDU tx/rx loop into own struct for better testing/fuzzing?
pub struct ClientInternals<
    const MAX_FRAMES: usize,
    const MAX_PDU_DATA: usize,
    const MAX_SLAVES: usize,
    TIMEOUT,
> {
    // TODO: un-pub
    pub pdu_loop: PduLoop<MAX_FRAMES, MAX_PDU_DATA, TIMEOUT>,
    _timeout: PhantomData<TIMEOUT>,
    // TODO: un-pub
    pub slaves: RefCell<heapless::Vec<Slave, MAX_SLAVES>>,
}

unsafe impl<const MAX_FRAMES: usize, const MAX_PDU_DATA: usize, const MAX_SLAVES: usize, TIMEOUT>
    Sync for ClientInternals<MAX_FRAMES, MAX_PDU_DATA, MAX_SLAVES, TIMEOUT>
{
}

impl<const MAX_FRAMES: usize, const MAX_PDU_DATA: usize, const MAX_SLAVES: usize, TIMEOUT>
    ClientInternals<MAX_FRAMES, MAX_PDU_DATA, MAX_SLAVES, TIMEOUT>
where
    TIMEOUT: TimerFactory,
{
    pub fn new() -> Self {
        // MSRV: Make `N` a `u8` when `generic_const_exprs` is stablised
        assert!(
            MAX_FRAMES < u8::MAX.into(),
            "Packet indexes are u8s, so cache array cannot be any bigger than u8::MAX"
        );

        Self {
            pdu_loop: PduLoop::new(),
            slaves: RefCell::new(heapless::Vec::new()),
            _timeout: PhantomData,
        }
    }

    /// Detect slaves and set their configured station addresses.
    pub async fn init(&self) -> Result<(), Error> {
        // Reset everything
        {
            // Reset slaves to init
            self.bwr(
                RegisterAddress::AlControl,
                AlControl::reset().pack().unwrap(),
            )
            .await?;

            // TODO: Clear FMMUs
            // TODO: Clear SMs
        }

        // Each slave increments working counter, so we can use it as a total count of slaves
        let (_res, num_slaves) = self.brd::<u8>(RegisterAddress::Type).await?;

        if usize::from(num_slaves) > self.slaves.borrow().capacity() {
            return Err(Error::TooManySlaves);
        }

        // Make sure slave list is empty
        self.slaves.borrow_mut().truncate(0);

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

            // TODO: Unwrap
            self.slaves
                .borrow_mut()
                .push(Slave::new(address, slave_state.state))
                .unwrap();
        }

        Ok(())
    }

    // TODO: Move onto `Slave` struct and invert control, e.g. `slave.request_state(state, &client)`
    pub async fn request_slave_state(&self, slave_idx: usize, state: AlState) -> Result<(), Error> {
        // TODO: Unwrap
        // TODO: DRY into function
        let address = self
            .slaves
            .try_borrow()?
            .get(slave_idx)
            .ok_or_else(|| Error::SlaveNotFound(slave_idx))?
            .configured_address;

        debug!("Set state {} for slave address {:#04x}", state, address);

        // Send state request
        self.fpwr(
            address,
            RegisterAddress::AlControl,
            AlControl::new(state).pack().unwrap(),
        )
        .await?
        .wkc(1, "AL control")?;

        // TODO: Move these to consts/timeout config struct
        let wait_ms = 200;
        let delay_ms = 10;

        // TODO: Make this a reusable function? Closure? Struct?
        for _ in 0..(wait_ms / delay_ms) {
            let status = self
                .fprd::<AlControl>(address, RegisterAddress::AlStatus)
                .await?
                .wkc(1, "AL status")?;

            if status.state == state {
                return Ok(());
            }

            TIMEOUT::timer(core::time::Duration::from_millis(delay_ms)).await;
        }

        // TODO: Extract into separate method to get slave status code
        {
            let (status, _working_counter) = self
                .fprd::<AlStatusCode>(address, RegisterAddress::AlStatusCode)
                .await?;

            println!("{}", status);
        }

        Err(Error::Timeout)
    }

    // TODO: Move onto `Slave` struct and invert control, e.g. `slave.request_state(state, &client)`
    pub async fn read_eeprom(&self, slave_idx: u16, eeprom_address: u16) -> Result<u32, Error> {
        let slave_idx = usize::from(slave_idx);

        // TODO: Unwrap
        // TODO: DRY into function
        let slave_address = self
            .slaves
            .try_borrow()?
            .get(slave_idx)
            .ok_or_else(|| Error::SlaveNotFound(slave_idx))?
            .configured_address;

        // TODO: When moved onto slave, check error flags

        let setup = SiiRequest::read(eeprom_address);

        // Set up an SII read. This writes the control word and the register word after it
        self.fpwr(slave_address, RegisterAddress::SiiControl, setup.to_array())
            .await?
            .wkc(1, "SII read setup")?;

        // TODO: Configurable timeout
        let timeout = TIMEOUT::timer(core::time::Duration::from_millis(10));

        // TODO: Make this a reusable function? Closure? Struct?
        let res = async {
            loop {
                let control = self
                    .fprd::<SiiControl>(slave_address, RegisterAddress::SiiControl)
                    .await?
                    .wkc(1, "SII busy wait")?;

                debug!("Loop {:?}", control.busy);

                if control.busy == false {
                    info!("WE DID IT");
                    break Result::<(), Error>::Ok(());
                }

                // TODO: Configurable loop tick
                TIMEOUT::timer(core::time::Duration::from_millis(1)).await;
            }
        };

        futures_lite::pin!(res);

        match select(res, timeout).await {
            Either::Right((_timeout, _res)) => return Err(Error::Timeout),
            _ => (),
        }

        let data = self
            .fprd::<u32>(slave_address, RegisterAddress::SiiData)
            .await?
            .wkc(1, "SII data")?;

        Ok(data)
    }

    // TODO: Dedupe with write_service when refactoring allows
    async fn read_service<T>(&self, command: Command) -> Result<PduResponse<T>, PduError>
    where
        T: PduRead,
        <T as PduRead>::Error: core::fmt::Debug,
    {
        let pdu = self.pdu_loop.pdu_tx(command, &[], T::len().into()).await?;

        let res = T::try_from_slice(pdu.data.as_slice()).map_err(|e| {
            println!("{:?}", e);
            PduError::Decode
        })?;

        Ok((res, pdu.working_counter))
    }

    // TODO: Support different I and O types; some things can return different data
    async fn write_service<T>(&self, command: Command, value: T) -> Result<PduResponse<T>, PduError>
    where
        T: PduData,
        <T as PduRead>::Error: core::fmt::Debug,
    {
        let pdu = self
            .pdu_loop
            .pdu_tx(command, value.as_slice(), T::len().into())
            .await?;

        let res = T::try_from_slice(pdu.data.as_slice()).map_err(|e| {
            println!("{:?}", e);
            PduError::Decode
        })?;

        Ok((res, pdu.working_counter))
    }

    pub async fn brd<T>(&self, register: RegisterAddress) -> Result<PduResponse<T>, PduError>
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
    pub async fn bwr<T>(
        &self,
        register: RegisterAddress,
        value: T,
    ) -> Result<PduResponse<T>, PduError>
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
    ) -> Result<PduResponse<T>, PduError>
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
    ) -> Result<PduResponse<T>, PduError>
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
    ) -> Result<PduResponse<T>, PduError>
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
    ) -> Result<PduResponse<T>, PduError>
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
}
