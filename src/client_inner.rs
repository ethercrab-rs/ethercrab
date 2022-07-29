use crate::{
    al_control::AlControl,
    al_status::AlState,
    al_status_code::AlStatusCode,
    check_working_counter,
    client::PduResponse,
    command::Command,
    error::{Error, PduError},
    pdu::Pdu,
    register::RegisterAddress,
    slave::Slave,
    timer_factory::TimerFactory,
    PduData, PduRead, BASE_SLAVE_ADDR, ETHERCAT_ETHERTYPE, MASTER_ADDR,
};
use core::{
    cell::{BorrowMutError, RefCell, RefMut},
    marker::PhantomData,
    sync::atomic::{AtomicU8, Ordering},
    task::{Poll, Waker},
};
use futures::future::{select, Either};
use packed_struct::PackedStructSlice;
use smoltcp::wire::EthernetFrame;

#[derive(Debug, PartialEq)]
pub enum RequestState {
    Created,
    Waiting,
    Done,
}

// TODO: Use atomic_refcell crate
pub struct ClientInternals<
    const MAX_FRAMES: usize,
    const MAX_PDU_DATA: usize,
    const MAX_SLAVES: usize,
    TIMEOUT,
> {
    wakers: RefCell<[Option<Waker>; MAX_FRAMES]>,
    frames: RefCell<[Option<(RequestState, Pdu<MAX_PDU_DATA>)>; MAX_FRAMES]>,
    send_waker: RefCell<Option<Waker>>,
    idx: AtomicU8,
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
            wakers: RefCell::new([(); MAX_FRAMES].map(|_| None)),
            frames: RefCell::new([(); MAX_FRAMES].map(|_| None)),
            send_waker: RefCell::new(None),
            idx: AtomicU8::new(0),
            slaves: RefCell::new(heapless::Vec::new()),
            _timeout: PhantomData,
        }
    }

    /// Detect slaves and set their configured station addresses.
    pub async fn init(&self) -> Result<(), Error> {
        // Each slave increments working counter, so we can use it as a total count of slaves
        let (_res, num_slaves) = self.brd::<u8>(RegisterAddress::Type).await?;

        if usize::from(num_slaves) > self.slaves.borrow().capacity() {
            return Err(Error::TooManySlaves);
        }

        // Make sure slave list is empty
        self.slaves.borrow_mut().truncate(0);

        for slave_idx in 0..num_slaves {
            let address = BASE_SLAVE_ADDR + slave_idx;

            let (_, working_counter) = self
                .apwr(
                    slave_idx,
                    RegisterAddress::ConfiguredStationAddress,
                    address,
                )
                .await?;

            check_working_counter!(working_counter, 1, "set station address")?;

            let (slave_state, working_counter) = self
                .fprd::<AlControl>(address, RegisterAddress::AlStatus)
                .await?;

            check_working_counter!(working_counter, 1, "get AL status")?;

            // TODO: Unwrap
            self.slaves
                .borrow_mut()
                .push(Slave::new(address, slave_state.state))
                .unwrap();
        }

        Ok(())
    }

    pub async fn request_slave_state(&self, slave_idx: usize, state: AlState) -> Result<(), Error> {
        // TODO: Unwrap
        let address = self
            .slaves
            .try_borrow()?
            .get(slave_idx)
            .ok_or_else(|| Error::SlaveNotFound(slave_idx))?
            .configured_address;

        let value = AlControl::new(state);

        let mut buf = [0u8; 2];
        value.pack_to_slice(&mut buf).unwrap();

        debug!("Set state {} for slave address {:#04x}", state, address);

        // Send state request
        let (_, working_counter) = self.fpwr(address, RegisterAddress::AlControl, buf).await?;

        check_working_counter!(working_counter, 1, "AL control")?;

        let wait_ms = 200;
        let delay_ms = 10;

        for _ in 0..(wait_ms / delay_ms) {
            let (status, working_counter) = self
                .fprd::<AlControl>(address, RegisterAddress::AlStatus)
                .await?;

            check_working_counter!(working_counter, 1, "AL status")?;

            if status.state == state {
                return Ok(());
            }

            TIMEOUT::timer(core::time::Duration::from_millis(delay_ms)).await;
        }

        // TODO: Extract into separate method
        {
            let (status, _working_counter) = self
                .fprd::<AlStatusCode>(address, RegisterAddress::AlStatusCode)
                .await?;

            println!("{}", status);
        }

        Err(Error::Timeout)
    }

    pub fn set_send_waker(&self, waker: &Waker) {
        if self.send_waker.borrow().is_none() {
            self.send_waker.borrow_mut().replace(waker.clone());
        }
    }

    pub fn frames_mut(
        &self,
    ) -> Result<RefMut<'_, [Option<(RequestState, Pdu<MAX_PDU_DATA>)>; MAX_FRAMES]>, BorrowMutError>
    {
        self.frames.try_borrow_mut()
    }

    pub async fn pdu(
        &self,
        command: Command,
        data: &[u8],
        data_length: u16,
    ) -> Result<Pdu<MAX_PDU_DATA>, PduError> {
        // braces to ensure we don't hold the refcell across awaits
        let idx = {
            // TODO: Confirm Ordering enum
            let idx = self.idx.fetch_add(1, Ordering::Release) % MAX_FRAMES as u8;

            // We're receiving too fast or the receive buffer isn't long enough
            if self.frames.borrow()[usize::from(idx)].is_some() {
                return Err(PduError::IndexInUse);
            }

            let mut pdu = Pdu::<MAX_PDU_DATA>::new(command, data_length, idx);

            pdu.data = data.try_into().map_err(|_| PduError::TooLong)?;

            // TODO: Races
            self.frames.borrow_mut()[usize::from(idx)] = Some((RequestState::Created, pdu));

            if let Some(waker) = &*self.send_waker.borrow() {
                waker.wake_by_ref()
            }

            usize::from(idx)
        };

        // MSRV: Use core::future::poll_fn when `future_poll_fn ` is stabilised
        let res = futures_lite::future::poll_fn(|ctx| {
            // TODO: Races
            let mut frames = self.frames.borrow_mut();

            let res = frames
                .get_mut(idx)
                .map(|frame| match frame {
                    Some((RequestState::Done, _pdu)) => frame
                        .take()
                        .map(|(_state, pdu)| Poll::Ready(Ok(pdu)))
                        // We shouldn't ever get here because we're already matching against
                        // `Some()`, but the alternative is an `unwrap()` so let's not go there.
                        .unwrap_or(Poll::Pending),
                    _ => Poll::Pending,
                })
                .unwrap_or_else(|| Poll::Ready(Err(PduError::InvalidIndex(idx))));

            self.wakers.borrow_mut()[usize::from(idx)] = Some(ctx.waker().clone());

            res
        });

        // TODO: Configurable timeout
        let timeout = TIMEOUT::timer(core::time::Duration::from_micros(30_000));

        let res = match select(res, timeout).await {
            Either::Left((res, _timeout)) => res,
            Either::Right((_timeout, _res)) => return Err(PduError::Timeout),
        };

        res
    }

    // TODO: Return a result if index is out of bounds, or we don't have a waiting packet
    pub fn parse_response_ethernet_packet(&self, raw_packet: &[u8]) {
        let raw_packet = EthernetFrame::new_unchecked(raw_packet);

        // Look for EtherCAT packets whilst ignoring broadcast packets sent from self
        if raw_packet.ethertype() != ETHERCAT_ETHERTYPE || raw_packet.src_addr() == MASTER_ADDR {
            return ();
        }

        let (_rest, pdu) = Pdu::<MAX_PDU_DATA>::from_ethernet_payload::<nom::error::Error<&[u8]>>(
            &raw_packet.payload(),
        )
        .expect("Packet parse");

        let idx = pdu.index;

        let waker = self.wakers.borrow_mut()[usize::from(idx)].take();

        // Frame is ready; tell everyone about it
        if let Some(waker) = waker {
            // TODO: Borrow races
            if let Some((state, existing_pdu)) = self.frames.borrow_mut()[usize::from(idx)].as_mut()
            {
                pdu.is_response_to(existing_pdu).unwrap();

                *state = RequestState::Done;
                *existing_pdu = pdu
            } else {
                panic!("No waiting frame for response");
            }

            waker.wake()
        }
    }

    pub async fn brd<T>(&self, register: RegisterAddress) -> Result<PduResponse<T>, PduError>
    where
        T: PduRead,
        <T as PduRead>::Error: core::fmt::Debug,
    {
        let pdu = self
            .pdu(
                Command::Brd {
                    // Address is always zero when sent from master
                    address: 0,
                    register: register.into(),
                },
                // No input data; this is a read
                &[],
                T::len().try_into().expect("Length conversion"),
            )
            .await?;

        let res = T::try_from_slice(pdu.data.as_slice()).map_err(|e| {
            println!("{:?}", e);
            PduError::Decode
        })?;

        Ok((res, pdu.working_counter))
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
        let address = 0u16.wrapping_sub(address);

        let pdu = self
            .pdu(
                Command::Aprd {
                    address,
                    register: register.into(),
                },
                &[],
                T::len().try_into().expect("Length conversion"),
            )
            .await?;

        let res = T::try_from_slice(pdu.data.as_slice()).map_err(|e| {
            println!("{:?}", e);
            PduError::Decode
        })?;

        Ok((res, pdu.working_counter))
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
        let pdu = self
            .pdu(
                Command::Fprd {
                    address,
                    register: register.into(),
                },
                &[],
                T::len().try_into().expect("Length conversion"),
            )
            .await?;

        let res = T::try_from_slice(pdu.data.as_slice()).map_err(|e| {
            println!("{:?}", e);
            PduError::Decode
        })?;

        Ok((res, pdu.working_counter))
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
        let address = 0u16.wrapping_sub(address);

        let pdu = self
            .pdu(
                Command::Apwr {
                    address,
                    register: register.into(),
                },
                value.as_slice(),
                T::len().try_into().expect("Length conversion"),
            )
            .await?;

        let res = T::try_from_slice(pdu.data.as_slice()).map_err(|e| {
            println!("{:?}", e);
            PduError::Decode
        })?;

        Ok((res, pdu.working_counter))
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
        let pdu = self
            .pdu(
                Command::Fpwr {
                    address,
                    register: register.into(),
                },
                value.as_slice(),
                T::len().try_into().expect("Length conversion"),
            )
            .await?;

        let res = T::try_from_slice(pdu.data.as_slice()).map_err(|e| {
            println!("{:?}", e);
            PduError::Decode
        })?;

        Ok((res, pdu.working_counter))
    }
}
