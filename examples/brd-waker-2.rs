//! A refactor of brd-waker to contain TX/RX in a single task, only passing data/wakers.

use async_ctrlc::CtrlC;
use core::cell::RefCell;
use core::future::Future;
use core::marker::PhantomData;
use core::sync::atomic::{AtomicU8, Ordering};
use core::task::Poll;
use core::task::Waker;
use ethercrab::command::Command;
use ethercrab::frame::FrameError;
use ethercrab::pdu2::Pdu;
use ethercrab::register::RegisterAddress;
use ethercrab::{PduData, ETHERCAT_ETHERTYPE, MASTER_ADDR};
use futures_lite::FutureExt;
use pnet::datalink::{self, DataLinkReceiver, DataLinkSender};
use smol::LocalExecutor;
use smoltcp::wire::{EthernetAddress, EthernetFrame, EthernetProtocol};
use std::sync::Arc;

#[cfg(target_os = "windows")]
// ASRock NIC
// const INTERFACE: &str = "TODO";
// USB NIC
const INTERFACE: &str = "\\Device\\NPF_{DCEDC919-0A20-47A2-9788-FC57D0169EDB}";
#[cfg(not(target_os = "windows"))]
const INTERFACE: &str = "eth0";

fn main() {
    let local_ex = LocalExecutor::new();

    // let (mut tx, mut rx) = get_tx_rx();

    let ctrlc = CtrlC::new().expect("cannot create Ctrl+C handler?");

    futures_lite::future::block_on(local_ex.run(ctrlc.race(async {
        let client = WrappedClient::<16, 16, smol::Timer>::new();

        local_ex
            .spawn(client.tx_rx_task(INTERFACE).unwrap())
            .detach();

        let res = client.brd::<[u8; 1]>(RegisterAddress::Type).await.unwrap();
        println!("RESULT: {:#02x?}", res);
        let res = client.brd::<u16>(RegisterAddress::Build).await.unwrap();
        println!("RESULT: {:#04x?}", res);
    })));
}

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
struct WrappedClient<const MAX_FRAMES: usize, const MAX_PDU_DATA: usize, TIMEOUT> {
    client: Arc<Client<MAX_FRAMES, MAX_PDU_DATA, TIMEOUT>>,
}

impl<const MAX_FRAMES: usize, const MAX_PDU_DATA: usize, TIMEOUT>
    WrappedClient<MAX_FRAMES, MAX_PDU_DATA, TIMEOUT>
where
    TIMEOUT: TimerFactory + Send + 'static,
{
    fn new() -> Self {
        Self {
            client: Arc::new(Client::new()),
        }
    }

    // TODO: Proper error - there are a couple of unwraps in here
    fn tx_rx_task(&self, device: &str) -> Result<impl Future<Output = ()>, std::io::Error> {
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

                                let packet = pdu_to_ethernet(pdu, &mut packet_buf).unwrap();

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
                        if packet.ethertype() == EthernetProtocol::Unknown(0x88a4)
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

    pub async fn brd<T>(&self, register: RegisterAddress) -> Result<T, SendPduError>
    where
        T: PduData,
        <T as PduData>::Error: core::fmt::Debug,
    {
        self.client
            .pdu(
                Command::Brd {
                    // Address is always zero when sent from master
                    address: 0,
                    register: register.into(),
                },
                // No input data; this is a read
                &[],
            )
            .await
    }
}

// TODO: Use atomic_refcell crate
struct Client<const MAX_FRAMES: usize, const MAX_PDU_DATA: usize, TIMEOUT> {
    wakers: RefCell<[Option<Waker>; MAX_FRAMES]>,
    frames: RefCell<[Option<(RequestState, Pdu<MAX_PDU_DATA>)>; MAX_FRAMES]>,
    send_waker: RefCell<Option<Waker>>,
    idx: AtomicU8,
    _timeout: PhantomData<TIMEOUT>,
}

unsafe impl<const MAX_FRAMES: usize, const MAX_PDU_DATA: usize, TIMEOUT> Sync
    for Client<MAX_FRAMES, MAX_PDU_DATA, TIMEOUT>
{
}

trait TimerFactory: core::future::Future + Unpin {
    fn timer(duration: core::time::Duration) -> Self;
}

impl TimerFactory for smol::Timer {
    fn timer(duration: core::time::Duration) -> Self {
        Self::after(duration)
    }
}

impl<const MAX_FRAMES: usize, const MAX_PDU_DATA: usize, TIMEOUT>
    Client<MAX_FRAMES, MAX_PDU_DATA, TIMEOUT>
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

    // TODO: Send data
    pub async fn pdu<T>(&self, command: Command, _data: &[u8]) -> Result<T, SendPduError>
    where
        T: PduData,
        <T as PduData>::Error: core::fmt::Debug,
    {
        // braces to ensure we don't hold the refcell across awaits!!
        let idx = {
            // TODO: Confirm ordering
            let idx = self.idx.fetch_add(1, Ordering::Release) % MAX_FRAMES as u8;

            // We're receiving too fast or the receive buffer isn't long enough
            if self.frames.borrow()[usize::from(idx)].is_some() {
                // println!("Index {idx} is already in use");

                return Err(SendPduError::IndexInUse);
            }

            // println!("BRD {idx}");

            let data_length = T::len();

            let pdu = Pdu::<MAX_PDU_DATA>::new(command, data_length, idx);

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

        let res = match futures::future::select(res, timeout).await {
            futures::future::Either::Left((res, _timeout)) => res,
            futures::future::Either::Right((_timeout, _res)) => return Err(SendPduError::Timeout),
        };

        // println!("Raw data {:?}", res.data.as_slice());

        T::try_from_slice(res.data.as_slice()).map_err(|e| {
            println!("{:?}", e);
            SendPduError::Decode
        })
    }

    // TODO: Return a result if index is out of bounds, or we don't have a waiting packet
    pub fn parse_response_ethernet_frame(&self, ethernet_frame_payload: &[u8]) {
        let (_rest, pdu) = Pdu::<MAX_PDU_DATA>::from_ethernet_payload(&ethernet_frame_payload)
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

#[derive(Debug)]
pub enum SendPduError {
    Timeout,
    IndexInUse,
    Send,
    Decode,
    CreateFrame(smoltcp::Error),
    Encode(cookie_factory::GenError),
    Frame(FrameError),
}

// Returns written bytes
fn pdu_to_ethernet<'a, const N: usize>(
    pdu: &Pdu<N>,
    buf: &'a mut [u8],
) -> Result<&'a [u8], SendPduError> {
    let ethernet_len = EthernetFrame::<&[u8]>::buffer_len(pdu.frame_buf_len());

    // TODO: Return result if it's not long enough
    let buf = &mut buf[0..ethernet_len];

    let mut frame = EthernetFrame::new_checked(buf).map_err(SendPduError::CreateFrame)?;

    frame.set_src_addr(MASTER_ADDR);
    frame.set_dst_addr(EthernetAddress::BROADCAST);
    frame.set_ethertype(ETHERCAT_ETHERTYPE);

    pdu.write_ethernet_payload(&mut frame.payload_mut())
        .map_err(SendPduError::Frame)?;

    // println!(
    //     "Send {}",
    //     PrettyPrinter::<EthernetFrame<&'static [u8]>>::new("", &frame)
    // );

    let buf = frame.into_inner();

    Ok(buf)
}
