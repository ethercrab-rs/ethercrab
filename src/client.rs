use crate::{
    command::Command,
    pdu::{Pdu, PduError},
    register::RegisterAddress,
    timer_factory::TimerFactory,
    PduData, ETHERCAT_ETHERTYPE, MASTER_ADDR,
};
use core::{
    cell::RefCell,
    future::Future,
    marker::PhantomData,
    sync::atomic::{AtomicU8, Ordering},
    task::{Poll, Waker},
};
use futures::future::{select, Either};
use futures_lite::FutureExt;
use pnet::datalink::{self, DataLinkReceiver, DataLinkSender};
use smoltcp::wire::EthernetFrame;

pub type PduResponse<T> = (T, u16);

#[derive(Debug)]
enum RequestState {
    Created,
    Waiting,
    Done,
}

fn get_tx_rx(
    device: &str,
) -> Result<(Box<dyn DataLinkSender>, Box<dyn DataLinkReceiver>), std::io::Error> {
    let interfaces = datalink::interfaces();

    dbg!(&interfaces);

    let interface = interfaces
        .into_iter()
        .find(|interface| interface.name == device)
        .unwrap();

    dbg!(interface.mac);

    let (tx, rx) = match datalink::channel(&interface, Default::default()) {
        Ok(datalink::Channel::Ethernet(tx, rx)) => (tx, rx),
        // FIXME
        Ok(_) => panic!("Unhandled channel type"),
        Err(e) => return Err(e),
    };

    Ok((tx, rx))
}

#[derive(Clone)]
pub struct Client<const MAX_FRAMES: usize, const MAX_PDU_DATA: usize, TIMEOUT> {
    client: std::sync::Arc<ClientInternals<MAX_FRAMES, MAX_PDU_DATA, TIMEOUT>>,
}

impl<const MAX_FRAMES: usize, const MAX_PDU_DATA: usize, TIMEOUT>
    Client<MAX_FRAMES, MAX_PDU_DATA, TIMEOUT>
where
    TIMEOUT: TimerFactory + Send + 'static,
{
    pub fn new() -> Self {
        Self {
            client: std::sync::Arc::new(ClientInternals::new()),
        }
    }

    // TODO: Proper error - there are a couple of unwraps in here
    pub fn tx_rx_task(&self, device: &str) -> Result<impl Future<Output = ()>, std::io::Error> {
        let client_tx = self.client.clone();
        let client_rx = self.client.clone();

        let (mut tx, mut rx) = get_tx_rx(device)?;

        let tx_task = futures_lite::future::poll_fn::<(), _>(move |ctx| {
            if client_tx.send_waker.borrow().is_none() {
                client_tx
                    .send_waker
                    .borrow_mut()
                    .replace(ctx.waker().clone());
            }

            if let Ok(mut frames) = client_tx.frames.try_borrow_mut() {
                for request in frames.iter_mut() {
                    if let Some((state, pdu)) = request {
                        match state {
                            RequestState::Created => {
                                let mut packet_buf = [0u8; 1536];

                                let packet = pdu.to_ethernet_frame(&mut packet_buf).unwrap();

                                tx.send_to(packet, None).unwrap().expect("Send");

                                *state = RequestState::Waiting;
                            }
                            _ => (),
                        }
                    }
                }
            }

            Poll::Pending
        });

        let rx_task = smol::unblock(move || {
            loop {
                match rx.next() {
                    Ok(packet) => {
                        let packet = EthernetFrame::new_unchecked(packet);

                        // Look for EtherCAT packets whilst ignoring broadcast packets sent from self
                        if packet.ethertype() == ETHERCAT_ETHERTYPE
                            && packet.src_addr() != MASTER_ADDR
                        {
                            client_rx.parse_response_ethernet_frame(packet.payload());
                        }
                    }
                    Err(e) => {
                        // If an error occurs, we can handle it here
                        panic!("An error occurred while reading: {}", e);
                    }
                }
            }
        });

        Ok(tx_task.race(rx_task))
    }

    pub async fn brd<T>(&self, register: RegisterAddress) -> Result<PduResponse<T>, PduError>
    where
        T: PduData,
        <T as PduData>::Error: core::fmt::Debug,
    {
        let pdu = self
            .client
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
        T: PduData,
        <T as PduData>::Error: core::fmt::Debug,
    {
        let address = 0u16.wrapping_sub(address);

        let pdu = self
            .client
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
        T: PduData,
        <T as PduData>::Error: core::fmt::Debug,
    {
        let pdu = self
            .client
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
        <T as PduData>::Error: core::fmt::Debug,
    {
        let address = 0u16.wrapping_sub(address);

        let pdu = self
            .client
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
}

// TODO: Use atomic_refcell crate
struct ClientInternals<const MAX_FRAMES: usize, const MAX_PDU_DATA: usize, TIMEOUT> {
    wakers: RefCell<[Option<Waker>; MAX_FRAMES]>,
    frames: RefCell<[Option<(RequestState, Pdu<MAX_PDU_DATA>)>; MAX_FRAMES]>,
    send_waker: RefCell<Option<Waker>>,
    idx: AtomicU8,
    _timeout: PhantomData<TIMEOUT>,
}

unsafe impl<const MAX_FRAMES: usize, const MAX_PDU_DATA: usize, TIMEOUT> Sync
    for ClientInternals<MAX_FRAMES, MAX_PDU_DATA, TIMEOUT>
{
}

impl<const MAX_FRAMES: usize, const MAX_PDU_DATA: usize, TIMEOUT>
    ClientInternals<MAX_FRAMES, MAX_PDU_DATA, TIMEOUT>
where
    TIMEOUT: TimerFactory,
{
    fn new() -> Self {
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
            _timeout: PhantomData,
        }
    }

    async fn pdu(
        &self,
        command: Command,
        data: &[u8],
        data_length: u16,
    ) -> Result<Pdu<MAX_PDU_DATA>, PduError> {
        // braces to ensure we don't hold the refcell across awaits!!
        let idx = {
            // TODO: Confirm ordering
            let idx = self.idx.fetch_add(1, Ordering::Release) % MAX_FRAMES as u8;

            // We're receiving too fast or the receive buffer isn't long enough
            if self.frames.borrow()[usize::from(idx)].is_some() {
                // println!("Index {idx} is already in use");

                return Err(PduError::IndexInUse);
            }

            let mut pdu = Pdu::<MAX_PDU_DATA>::new(command, data_length, idx);

            pdu.data = data.try_into().map_err(|_| PduError::TooLong)?;

            self.frames.borrow_mut()[usize::from(idx)] = Some((RequestState::Created, pdu));

            // println!("TX waker? {:?}", self.send_waker);

            if let Some(waker) = &*self.send_waker.borrow() {
                waker.wake_by_ref()
            }

            usize::from(idx)
        };

        // MSRV: Use core::future::poll_fn when `future_poll_fn ` is stabilised
        let res = futures_lite::future::poll_fn(|ctx| {
            let frames = self.frames.try_borrow_mut();

            let res = if let Ok(mut frames) = frames {
                let frame = frames[usize::from(idx)].take();

                match frame {
                    Some((RequestState::Done, pdu)) => Poll::Ready(pdu),
                    // Not ready yet, put the request back.
                    // TODO: This is dumb, we just want a reference
                    Some(state) => {
                        frames[usize::from(idx)] = Some(state);
                        Poll::Pending
                    }
                    _ => Poll::Pending,
                }
            } else {
                // Using the failed borrow on `self.frames` as a sentinel, we can assume packets are
                // being sent/received so we'll do nothing for now
                Poll::Pending
            };

            self.wakers.borrow_mut()[usize::from(idx)] = Some(ctx.waker().clone());

            res
        });

        // TODO: Configurable timeout
        let timeout = TIMEOUT::timer(core::time::Duration::from_micros(30_000));

        let res = match select(res, timeout).await {
            Either::Left((res, _timeout)) => res,
            Either::Right((_timeout, _res)) => return Err(PduError::Timeout),
        };

        // println!("Raw data {:?}", res.data.as_slice());

        Ok(res)
    }

    // TODO: Return a result if index is out of bounds, or we don't have a waiting packet
    pub fn parse_response_ethernet_frame(&self, ethernet_frame_payload: &[u8]) {
        let (_rest, pdu) = Pdu::<MAX_PDU_DATA>::from_ethernet_payload::<nom::error::Error<&[u8]>>(
            &ethernet_frame_payload,
        )
        .expect("Packet parse");

        let idx = pdu.index;

        let waker = self.wakers.borrow_mut()[usize::from(idx)].take();

        // println!("Looking for waker #{idx}: {:?}", waker);

        // Frame is ready; tell everyone about it
        if let Some(waker) = waker {
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
}
