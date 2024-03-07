//! Utilities to replay Wireshark captures as part of regression/integration tests.

use ethercrab::{
    error::Error,
    internals::{FrameHeader, PduHeader},
    std::tx_rx_task,
    PduRx, PduTx,
};
use ethercrab_wire::EtherCrabWireRead;
use pcap_file::pcapng::{Block, PcapNgReader};
use smoltcp::wire::{EthernetAddress, EthernetFrame};
use std::{
    collections::{HashMap, VecDeque},
    fs::File,
    future::Future,
    hash::Hasher,
    pin::Pin,
    task::Poll,
};

/// Combined EtherCAT and PDU headers.
///
/// Only supports a PDU per EtherCAT frame.
#[derive(Debug, Copy, Clone, ethercrab_wire::EtherCrabWireRead)]
#[wire(bytes = 12)]
pub struct FramePreamble {
    #[wire(bytes = 2)]
    header: FrameHeader,

    #[wire(bytes = 10)]
    pdu_header: PduHeader,
}

pub fn spawn_tx_rx(capture_file_path: &str, tx: PduTx<'static>, rx: PduRx<'static>) {
    let interface = std::env::var("INTERFACE");

    // If INTERFACE env var is present, run using real hardware
    if let Ok(interface) = interface {
        log::info!("Running using real hardware on interface {}", interface);

        tokio::spawn(tx_rx_task(&interface, tx, rx).expect("spawn TX/RX task"));
    }
    // Otherwise, use mocked TX/RX task
    else {
        log::info!("Running dummy TX/RX loop");

        tokio::spawn(dummy_tx_rx_task(capture_file_path, tx, rx).expect("Dummy spawn"));
    };
}

const MASTER_ADDR: EthernetAddress = EthernetAddress([0x10, 0x10, 0x10, 0x10, 0x10, 0x10]);
const REPLY_ADDR: EthernetAddress = EthernetAddress([0x12, 0x10, 0x10, 0x10, 0x10, 0x10]);

#[derive(Debug)]
struct PreambleHash(pub FramePreamble);

impl Eq for PreambleHash {}

impl PartialEq for PreambleHash {
    fn eq(&self, other: &Self) -> bool {
        self.0
            .pdu_header
            .test_only_hacked_equal(&other.0.pdu_header)
            && self.0.header == other.0.header
    }
}

impl core::hash::Hash for PreambleHash {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.pdu_header.test_only_hacked_hash(state);
        self.0.header.hash(state);
    }
}

struct DummyTxRxFut<'a> {
    tx: PduTx<'a>,
    rx: PduRx<'a>,
    // The hashmap here is an optimisation over just a straight vec to improve popping performance.
    pdu_sends: HashMap<PreambleHash, VecDeque<(EthernetFrame<Vec<u8>>, usize)>>,
    pdu_responses: HashMap<PreambleHash, VecDeque<(EthernetFrame<Vec<u8>>, usize)>>,
}

impl Future for DummyTxRxFut<'_> {
    type Output = Result<(), Error>;

    fn poll(mut self: Pin<&mut Self>, ctx: &mut core::task::Context<'_>) -> Poll<Self::Output> {
        self.tx.replace_waker(ctx.waker());

        let mut buf = [0u8; 1536];

        while let Some(frame) = self.tx.next_sendable_frame() {
            // let expected = self.next_line_is_send();

            let mut sent_preamble = None;

            frame
                .send_blocking(&mut buf, |got| {
                    let frame = EthernetFrame::new_unchecked(got);

                    let got_preamble = FramePreamble::unpack_from_slice(frame.payload())
                        .map(PreambleHash)
                        .expect("Bad preamble");

                    let (expected, tx_packet_number) = self
                        .pdu_sends
                        .get_mut(&got_preamble)
                        .expect("Sent preamble not found in dump")
                        .pop_front()
                        .expect("Not enough packets for this preamble");

                    assert_eq!(
                        expected.as_ref(),
                        got,
                        "TX line {}, search header {:?}",
                        tx_packet_number,
                        got_preamble
                    );

                    sent_preamble = Some(got_preamble);

                    Ok(got.len())
                })
                .expect("Failed to send");

            let sent_preamble = sent_preamble.expect("No send preamble");

            let (expected, _rx_packet_number) = self
                .pdu_responses
                .get_mut(&sent_preamble)
                .expect("Receive preamble not found in dump")
                .pop_front()
                .expect("Not enough packets for this preamble");

            self.rx.receive_frame(expected.as_ref()).expect("Frame RX")
        }

        Poll::Pending
    }
}

/// Spawn a TX and RX task.
pub fn dummy_tx_rx_task(
    capture_file_path: &str,
    pdu_tx: PduTx<'static>,
    pdu_rx: PduRx<'static>,
) -> Result<impl Future<Output = Result<(), Error>>, std::io::Error> {
    // let file_in = File::open(capture_file_path).expect("Error opening file");
    let file_in2 = File::open(capture_file_path).expect("Error opening file");
    // let pcapng_reader = PcapNgReader::new(file_in).expect("Failed to init PCAP reader");
    let mut pcapng_reader2 = PcapNgReader::new(file_in2).expect("Failed to init PCAP reader");

    let mut packet_number = 0;
    let mut pdu_responses = HashMap::new();
    let mut pdu_sends = HashMap::new();

    while let Some(block) = pcapng_reader2.next_block() {
        // Indexed from 1 in the Wireshark UI
        packet_number += 1;

        // Check if there is no error
        let block = block.expect("Block error");

        let (raw, preamble) = match block {
            Block::EnhancedPacket(block) => {
                let buf = block.data.to_owned();

                let buf2 = buf.iter().copied().collect::<Vec<_>>();

                let mut f = EthernetFrame::new_checked(buf2).expect("Failed to parse block");

                assert_eq!(
                    u16::from(f.ethertype()),
                    0x88a4,
                    "packet {} is not an EtherCAT frame",
                    packet_number
                );

                let preamble = FramePreamble::unpack_from_slice(f.payload_mut())
                    .map(PreambleHash)
                    .expect("Invalid frame header");

                (f, preamble)
            }
            Block::InterfaceDescription(_) | Block::InterfaceStatistics(_) => continue,
            other => panic!("Frame {} is not correct type: {:?}", packet_number, other),
        };

        if raw.src_addr() == MASTER_ADDR {
            pdu_sends
                .entry(preamble)
                .or_insert(VecDeque::new())
                .push_back((raw, packet_number));
        } else if raw.src_addr() == REPLY_ADDR {
            pdu_responses
                .entry(preamble)
                .or_insert(VecDeque::new())
                .push_back((raw, packet_number));
        } else {
            panic!(
                "Frame {} does not have EtherCAT address (has {:?} instead)",
                packet_number,
                raw.src_addr()
            );
        }
    }

    let task = DummyTxRxFut {
        tx: pdu_tx,
        rx: pdu_rx,
        pdu_sends,
        pdu_responses,
    };

    Ok(task)
}
