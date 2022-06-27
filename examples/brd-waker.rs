//! Similar to waker-list, it keeps a list of wakers, but this time will send a `BRD` service and
//! listen for its response by parsing the frame.

use chrono::Utc;
use core::cell::{Cell, RefCell};
use core::mem;
use core::sync::atomic::{AtomicU8, Ordering};
use core::task::Poll;
use core::task::Waker;
use core::time::Duration;
use ethercrab::pdu2::Pdu;
use heapless::FnvIndexMap;
use mac_address::{get_mac_address, MacAddress};
use pcap::PacketHeader;
use pnet::{
    datalink::{self, DataLinkReceiver, DataLinkSender},
    packet::{ethernet::EthernetPacket, Packet},
};
use smol::LocalExecutor;
use smoltcp::wire::{EthernetAddress, EthernetFrame, EthernetProtocol, PrettyPrinter};
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::{io, path::PathBuf};

#[cfg(target_os = "windows")]
const INTERFACE: &str = "\\Device\\NPF_{0D792EC2-0E89-4AB6-BE39-3F41EC42AEA3}";
#[cfg(not(target_os = "windows"))]
const INTERFACE: &str = "eth0";

fn get_tx_rx() -> (Box<dyn DataLinkSender>, Box<dyn DataLinkReceiver>) {
    let interfaces = datalink::interfaces();

    dbg!(&interfaces);

    let interface = interfaces
        .into_iter()
        .find(|interface| dbg!(&interface.name) == INTERFACE)
        .unwrap();

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

    futures_lite::future::block_on(local_ex.run(async {
        let client = Arc::new(Client::<16, 16>::default());

        let mut client2 = client.clone();
        // let mut client3 = client.clone();

        // local_ex
        //     .spawn(async move {
        //     //
        //     })
        //     .detach();

        let handle = thread::spawn(move || {
            loop {
                match rx.next() {
                    Ok(packet) => {
                        let packet = EthernetFrame::new_unchecked(packet);

                        if packet.ethertype() == EthernetProtocol::Unknown(0x88a4) {
                            println!("EtherCAT packet received");

                            {
                                client2.parse_response_ethernet_frame(packet.payload());
                            }
                        }
                    }
                    Err(e) => {
                        // If an error occurs, we can handle it here
                        panic!("An error occurred while reading: {}", e);
                    }
                }
            }
        });

        async_io::Timer::after(Duration::from_millis(1000)).await;
        let res = client.brd::<[u8; 1]>(0x0000, &mut tx).await.unwrap();
        println!("RESULT: {:?}", res);
        async_io::Timer::after(Duration::from_millis(1000)).await;

        handle.join().unwrap();
    }));
}

// #[derive(Clone)]
struct Client<const N: usize, const D: usize> {
    wakers: RefCell<FnvIndexMap<u8, Waker, N>>,
    frames: RefCell<FnvIndexMap<u8, Pdu<D>, N>>,
    idx: AtomicU8,
}

// FIXME: This causes interleaved debug output among other things. Not cool.
//
//
//
//
// INCREDIBLY BROKEN. STOP USING.
//
//
//
//
//
// unsafe impl<const N: usize, const D: usize> Send for Client<D, N> {}
// unsafe impl<const N: usize, const D: usize> Sync for Client<D, N> {}

impl<const N: usize, const D: usize> Default for Client<N, D> {
    fn default() -> Self {
        Self {
            wakers: RefCell::new(FnvIndexMap::new()),
            frames: RefCell::new(FnvIndexMap::new()),
            idx: AtomicU8::new(0),
        }
    }
}

impl<const N: usize, const D: usize> Client<N, D> {
    // TODO: Register address enum ETG1000.4 Table 31
    // TODO: Make `tx` a trait somehow so we can use it in both no_std and std
    pub async fn brd<T>(&self, address: u16, tx: &mut Box<dyn DataLinkSender>) -> Result<T, ()>
    where
        for<'a> T: TryFrom<&'a [u8]>,
        for<'a> <T as TryFrom<&'a [u8]>>::Error: core::fmt::Debug,
    {
        // TODO: Wrapping/saturating add for `N`
        let idx = self.idx.fetch_add(1, Ordering::Release);

        println!("BRD {idx}");

        let pdu = Pdu::<1>::brd(address);

        let ethernet_frame = pdu_to_ethernet(&pdu);

        tx.send_to(&ethernet_frame.as_ref(), None)
            .unwrap()
            .expect("Send");

        let res = futures_lite::future::poll_fn(|ctx| {
            let removed = { self.frames.borrow_mut().remove(&idx) };

            println!("poll_fn idx {} {}", idx, removed.is_some());

            if let Some(frame) = removed {
                println!("poll_fn -> Ready, data {:?}", frame.data);
                Poll::Ready(frame)
            } else {
                self.wakers
                    .borrow_mut()
                    .insert(idx, ctx.waker().clone())
                    .expect("Failed to insert waker");

                println!("poll_fn -> Pending");

                Poll::Pending
            }
        })
        .await;

        dbg!(res.data.as_slice()).try_into().map_err(|e| {
            println!("{:?}", e);
            ()
        })
    }

    pub fn parse_response_ethernet_frame(&self, ethernet_frame_payload: &[u8]) {
        // let idx = ethernet_frame_payload[0];

        println!("Parse response {:?}", ethernet_frame_payload);

        let (rest, pdu) =
            Pdu::<D>::from_ethercat_frame_unchecked(&ethernet_frame_payload).expect("Packet parse");

        // TODO: Handle multiple PDUs here
        if !rest.is_empty() {
            println!("{} remaining bytes! (maybe just padding)", rest.len());
        }

        println!("Received PDU {:#?}", pdu);
        let idx = pdu.index;

        // println!("Got ethernet frame {idx}");

        let waker = { self.wakers.borrow_mut().remove(&idx) };

        println!("Looking for waker #{idx}: {:?}", waker);

        // Frame is ready; tell everyone about it
        if let Some(waker) = waker {
            println!("I have a waker {}, data {:?}", idx, pdu.data);

            // TODO: Validate PDU against the one stored with the waker.

            self.frames
                .borrow_mut()
                .insert(idx, pdu)
                // .get_mut(&idx)
                // .map(|old| {
                //     old.data = pdu.data;
                //     old.register_address = pdu.register_address;
                //     old.working_counter = pdu.working_counter;
                // })
                .expect("Failed to insert");
            waker.wake()
        }
    }

    // DELETEME
    pub fn current_idx(&self) -> u8 {
        self.idx.load(Ordering::Relaxed).saturating_sub(1)
    }
}

// TODO: Move into crate, pass buffer in instead of returning a vec
fn pdu_to_ethernet<const N: usize>(pdu: &Pdu<N>) -> EthernetFrame<Vec<u8>> {
    let src = get_mac_address()
        .expect("Failed to read MAC")
        .expect("No mac found");

    // Broadcast
    let dest = MacAddress::default();

    let ethernet_len = EthernetFrame::<&[u8]>::buffer_len(pdu.frame_buf_len());

    let mut buffer = Vec::new();
    buffer.resize(ethernet_len, 0x00u8);

    let mut frame = EthernetFrame::new_checked(buffer).unwrap();

    pdu.as_ethercat_frame(&mut frame.payload_mut()).unwrap();
    frame.set_src_addr(EthernetAddress::from_bytes(&src.bytes()));
    frame.set_dst_addr(EthernetAddress::from_bytes(&dest.bytes()));
    // TODO: Const
    frame.set_ethertype(EthernetProtocol::Unknown(0x88a4));

    println!(
        "Send {}",
        PrettyPrinter::<EthernetFrame<&'static [u8]>>::new("", &frame)
    );

    frame
}
