//! Similar to waker-list, it keeps a list of wakers, but this time will send a `BRD` service and
//! listen for its response by parsing the frame.

use async_ctrlc::CtrlC;
use core::cell::RefCell;
use core::sync::atomic::{AtomicU8, Ordering};
use core::task::Poll;
use core::task::Waker;
use ethercrab::command::Command;
use ethercrab::frame::FrameError;
use ethercrab::pdu::Pdu;
use ethercrab::register::RegisterAddress;
use ethercrab::{PduData, ETHERCAT_ETHERTYPE, MASTER_ADDR};
use futures_lite::FutureExt;
use pnet::datalink::{self, DataLinkReceiver, DataLinkSender};
use smol::LocalExecutor;
use smoltcp::wire::{EthernetAddress, EthernetFrame, EthernetProtocol, PrettyPrinter};
use std::marker::PhantomData;
use std::sync::Arc;

#[cfg(target_os = "windows")]
// ASRock NIC
// const INTERFACE: &str = "TODO";
// USB NIC
const INTERFACE: &str = "\\Device\\NPF_{DCEDC919-0A20-47A2-9788-FC57D0169EDB}";
#[cfg(not(target_os = "windows"))]
const INTERFACE: &str = "eth0";

fn get_tx_rx() -> (Box<dyn DataLinkSender>, Box<dyn DataLinkReceiver>) {
    let interfaces = datalink::interfaces();

    dbg!(&interfaces);

    let interface = interfaces
        .into_iter()
        .find(|interface| interface.name == INTERFACE)
        .unwrap();

    dbg!(interface.mac);

    let (tx, rx) = match datalink::channel(&interface, Default::default()) {
        Ok(datalink::Channel::Ethernet(tx, rx)) => (tx, rx),
        Ok(_) => panic!("Unhandled channel type"),
        Err(e) => panic!(
            "An error occurred when creating the datalink channel: {}",
            e
        ),
    };

    (tx, rx)
}

fn main() {
    let local_ex = LocalExecutor::new();

    let (mut tx, mut rx) = get_tx_rx();

    let ctrlc = CtrlC::new().expect("cannot create Ctrl+C handler?");

    futures_lite::future::block_on(local_ex.run(ctrlc.race(async {
        let client = Arc::new(Client::<16, 16, smol::Timer>::new());
        let client2 = client.clone();

        smol::spawn(smol::unblock(move || {
            loop {
                match rx.next() {
                    Ok(packet) => {
                        let packet = EthernetFrame::new_unchecked(packet);

                        if packet.ethertype() == EthernetProtocol::Unknown(0x88a4) {
                            // Ignore broadcast packets sent from self
                            if packet.src_addr() == MASTER_ADDR {
                                continue;
                            }

                            println!(
                                "Received EtherCAT packet. Source MAC {}, dest MAC {}",
                                packet.src_addr(),
                                packet.dst_addr()
                            );

                            client2.parse_response_ethernet_frame(packet.payload());
                        }
                    }
                    Err(e) => {
                        // If an error occurs, we can handle it here
                        panic!("An error occurred while reading: {}", e);
                    }
                }
            }
        }))
        .detach();

        let res = client
            .brd::<[u8; 1]>(RegisterAddress::Type, &mut tx)
            .await
            .unwrap();
        println!("RESULT: {:#02x?}", res);
        let res = client
            .brd::<u16>(RegisterAddress::Build, &mut tx)
            .await
            .unwrap();
        println!("RESULT: {:#04x?}", res);
    })));
}

struct Client<const MAX_FRAMES: usize, const MAX_PDU_DATA: usize, TIMEOUT> {
    wakers: RefCell<[Option<Waker>; MAX_FRAMES]>,
    frames: RefCell<[Option<heapless::Vec<u8, MAX_PDU_DATA>>; MAX_FRAMES]>,
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
            idx: AtomicU8::new(0),
            _timeout: PhantomData,
        }
    }

    // TODO: Make `tx` a trait somehow so we can use it in both no_std and std
    pub async fn brd<T>(
        &self,
        address: RegisterAddress,
        tx: &mut Box<dyn DataLinkSender>,
    ) -> Result<T, SendPduError>
    where
        T: PduData,
        <T as PduData>::Error: core::fmt::Debug,
    {
        let address = address as u16;
        let idx = self.idx.fetch_add(1, Ordering::Release) % MAX_FRAMES as u8;

        // We're receiving too fast or the receive buffer isn't long enough
        if self.frames.borrow()[usize::from(idx)].is_some() {
            println!("Index {idx} is already in use");

            return Err(SendPduError::IndexInUse);
        }

        println!("BRD {idx}");

        let data_length = T::len();

        let pdu = Pdu::<MAX_PDU_DATA>::new(
            Command::Brd {
                address: 0,
                register: address,
            },
            data_length,
            idx,
        );

        let mut ethernet_buf = [0x00u8; 1536];

        let ethernet_frame = pdu_to_ethernet(&pdu, &mut ethernet_buf)?;

        tx.send_to(&ethernet_frame.as_ref(), None)
            .ok_or_else(|| SendPduError::Send)?
            .map_err(|_| SendPduError::Send)?;

        let res = futures_lite::future::poll_fn(|ctx| {
            let removed = self.frames.borrow_mut()[usize::from(idx)].take();

            println!("poll_fn idx {} has data {:?}", idx, removed);

            if let Some(frame) = removed {
                println!("poll_fn -> Ready, data {:?}", frame);

                assert_eq!(frame.len(), data_length as usize);

                Poll::Ready(frame)
            } else {
                self.wakers.borrow_mut()[usize::from(idx)].replace(ctx.waker().clone());

                println!("poll_fn -> Pending, waker #{idx}");

                Poll::Pending
            }
        });

        // TODO: Configurable timeout
        let timeout = TIMEOUT::timer(core::time::Duration::from_micros(30_000));

        let res = match futures::future::select(res, timeout).await {
            futures::future::Either::Left((res, _timeout)) => res,
            futures::future::Either::Right((_timeout, _res)) => return Err(SendPduError::Timeout),
        };

        println!("Raw data {:?}", res.as_slice());

        T::try_from_slice(res.as_slice()).map_err(|e| {
            println!("{:?}", e);
            SendPduError::Decode
        })
    }

    // TODO: Return a result if index is out of bounds, or we don't have a waiting packet
    pub fn parse_response_ethernet_frame(&self, ethernet_frame_payload: &[u8]) {
        let (rest, pdu) = Pdu::<MAX_PDU_DATA>::from_ethernet_payload(&ethernet_frame_payload)
            .expect("Packet parse");

        // TODO: Handle multiple PDUs here
        if !rest.is_empty() {
            println!("{} remaining bytes! (maybe just padding)", rest.len());
        }

        let idx = pdu.index;

        let waker = self.wakers.borrow_mut()[usize::from(idx)].take();

        println!("Looking for waker #{idx}: {:?}", waker);

        // Frame is ready; tell everyone about it
        if let Some(waker) = waker {
            // TODO: Validate PDU against the one stored with the waker.

            println!("Waker #{idx} found. Insert PDU data {:?}", pdu);

            self.frames.borrow_mut()[usize::from(idx)].replace(pdu.data);
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

    println!(
        "Send {}",
        PrettyPrinter::<EthernetFrame<&'static [u8]>>::new("", &frame)
    );

    let buf = frame.into_inner();

    Ok(buf)
}
