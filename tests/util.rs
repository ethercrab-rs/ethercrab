//! Utilities to replay Wireshark captures as part of regression/integration tests.

use ethercrab::{
    error::Error,
    internals::{EthernetAddress, EthernetFrame},
    std::tx_rx_task,
    PduRx, PduTx, ReceiveAction,
};
use pcap_file::pcapng::{Block, PcapNgReader};
use std::{
    collections::{HashMap, VecDeque},
    fs::File,
    future::Future,
    hash::Hasher,
    io::{BufRead, BufReader},
    path::PathBuf,
    pin::Pin,
    task::Poll,
    time::Duration,
};

#[allow(unused)]
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

        let file_in2 = File::open(capture_file_path).expect("Error opening file");

        let reader = BufReader::new(file_in2);

        tokio::spawn(dummy_tx_rx_task(reader, tx, rx, None, None).expect("Dummy spawn"));
    };
}

#[allow(unused)]
pub fn spawn_tx_rx_for_miri(
    capture_file_bytes: &'static [u8],
    tx: PduTx<'static>,
    rx: PduRx<'static>,
    cache: Option<&[u8]>,
    cache_filename: PathBuf,
) {
    let interface = std::env::var("INTERFACE");

    // If INTERFACE env var is present, run using real hardware
    if let Ok(_) = interface {
        log::error!("This method can only be used in mock mode");

        panic!()
    }
    // Otherwise, use mocked TX/RX task
    else {
        log::info!("Running dummy TX/RX loop");

        let reader = BufReader::new(capture_file_bytes);

        tokio::spawn(
            dummy_tx_rx_task(reader, tx, rx, cache, Some(cache_filename)).expect("Dummy spawn"),
        );
    };
}

const MASTER_ADDR: EthernetAddress = EthernetAddress([0x10, 0x10, 0x10, 0x10, 0x10, 0x10]);
const REPLY_ADDR: EthernetAddress = EthernetAddress([0x12, 0x10, 0x10, 0x10, 0x10, 0x10]);

#[derive(Debug, Clone, savefile_derive::Savefile)]
struct PreambleHash(pub [u8; 12]);

impl Eq for PreambleHash {}

impl PartialEq for PreambleHash {
    fn eq(&self, other: &Self) -> bool {
        let command_code = self.0[2];
        let other_command_code = other.0[2];
        let index = self.0[3];
        let other_index = other.0[3];
        let command_raw = &self.0[4..8];
        let other_command_raw = &other.0[4..8];
        let irq = &self.0[10..12];
        let other_irq = &other.0[10..12];

        // Check EtherCAT header
        self.0[0..2] == other.0[0..2]
            && command_code == other_command_code
            && index == other_index
            && if matches!(command_code, 4 | 5) {
                command_raw == other_command_raw
            } else {
                true
            }
            && irq == other_irq
    }
}

impl core::hash::Hash for PreambleHash {
    fn hash<H: Hasher>(&self, state: &mut H) {
        let command_code = self.0[2];
        let index = self.0[3];
        let command_raw = &self.0[4..8];

        command_code.hash(state);
        index.hash(state);

        if matches!(command_code, 4 | 5) {
            command_raw.hash(state)
        }
    }
}

struct DummyTxRxFut<'a> {
    tx: PduTx<'a>,
    rx: PduRx<'a>,
    // The map here is an optimisation over just a straight vec to improve popping performance.
    pdu_sends: HashMap<PreambleHash, VecDeque<(Vec<u8>, usize)>>,
    pdu_responses: HashMap<PreambleHash, VecDeque<(Vec<u8>, usize)>>,
}

impl Future for DummyTxRxFut<'_> {
    type Output = Result<ReceiveAction, Error>;

    fn poll(mut self: Pin<&mut Self>, ctx: &mut core::task::Context<'_>) -> Poll<Self::Output> {
        self.tx.replace_waker(ctx.waker());

        while let Some(frame) = self.tx.next_sendable_frame() {
            let mut sent_preamble = None;

            frame
                .send_blocking(|got| {
                    let frame = EthernetFrame::new_unchecked(got);

                    let got_preamble = PreambleHash(frame.payload()[0..12].try_into().unwrap());

                    let (expected, tx_packet_number) = self
                        .pdu_sends
                        .get_mut(&got_preamble)
                        .expect("Sent preamble not found in dump")
                        .pop_front()
                        .expect("Not enough packets for this preamble");

                    assert_eq!(
                        &expected, got,
                        "TX line {}, search header {:?}",
                        tx_packet_number, got_preamble
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

            // A representative reasonably good RTT for a Linux machine
            std::thread::sleep(Duration::from_micros(50));

            while let Err(_) = self.rx.receive_frame(expected.as_ref()) {}
        }

        Poll::Pending
    }
}

/// Spawn a TX and RX task.
pub fn dummy_tx_rx_task(
    capture_file: impl BufRead,
    pdu_tx: PduTx<'static>,
    pdu_rx: PduRx<'static>,
    cache: Option<&[u8]>,
    cache_filename: Option<PathBuf>,
) -> Result<impl Future<Output = Result<ReceiveAction, Error>>, std::io::Error> {
    #[derive(savefile_derive::Savefile)]
    struct Cache {
        pdu_sends: HashMap<PreambleHash, VecDeque<(Vec<u8>, usize)>>,
        pdu_responses: HashMap<PreambleHash, VecDeque<(Vec<u8>, usize)>>,
    }

    if let Some(cache) = cache {
        log::debug!("Has cache");

        let cache: Cache = savefile::load_from_mem(cache, 0).unwrap();

        log::debug!(
            "--> Loaded {} sends, {} receives",
            cache.pdu_sends.len(),
            cache.pdu_responses.len()
        );

        return Ok(DummyTxRxFut {
            tx: pdu_tx,
            rx: pdu_rx,
            pdu_sends: cache.pdu_sends,
            pdu_responses: cache.pdu_responses,
        });
    }

    let mut pcapng_reader = PcapNgReader::new(capture_file).expect("Failed to init PCAP reader");

    log::debug!("Start parsing PCAP file");

    let mut packet_number = 0;
    let mut blocks = Vec::new();

    while let Some(block) = pcapng_reader.next_block().and_then(|res| res.ok()) {
        // Indexed from 1 in the Wireshark UI
        packet_number += 1;

        if packet_number % 100 == 0 {
            log::debug!("Packet {}", packet_number);
        }

        match block {
            Block::EnhancedPacket(block) => {
                blocks.push(block.into_owned());
            }
            Block::InterfaceDescription(_) | Block::InterfaceStatistics(_) => continue,
            other => panic!(
                "Frame {:#04x} is not correct type: {:?}",
                packet_number, other
            ),
        };
    }

    println!("");

    log::debug!("Finished reading PCAP file");

    let mut pdu_responses = HashMap::with_capacity(blocks.len());
    let mut pdu_sends = HashMap::with_capacity(blocks.len());

    for (packet_number, src_addr, raw, preamble) in
        blocks
            .into_iter()
            .enumerate()
            .map(|(packet_number, block)| {
                // 1-indexed to match Wireshark UI
                let packet_number = packet_number + 1;

                let buf = block.data.into_owned();

                let mut f = EthernetFrame::new_checked(buf).expect("Failed to parse block");

                assert_eq!(
                    u16::from(f.ethertype()),
                    0x88a4,
                    "packet {} is not an EtherCAT frame",
                    packet_number,
                );

                let preamble = PreambleHash(f.payload_mut()[0..12].try_into().unwrap());

                (packet_number, f.src_addr(), f.into_inner(), preamble)
            })
    {
        if packet_number % 100 == 0 {
            log::debug!("Grouped {} blocks", packet_number);
        }

        if src_addr == MASTER_ADDR {
            pdu_sends
                .entry(preamble)
                .or_insert(VecDeque::new())
                .push_back((raw, packet_number));
        } else if src_addr == REPLY_ADDR {
            pdu_responses
                .entry(preamble)
                .or_insert(VecDeque::new())
                .push_back((raw, packet_number));
        } else {
            panic!(
                "Frame {:#04x} does not have EtherCAT address (has {:?} instead)",
                packet_number, src_addr
            );
        }
    }

    log::debug!("Done grouping blocks");

    let task = if let Some(cache_path) = cache_filename {
        let cache = Cache {
            pdu_sends: pdu_sends,
            pdu_responses: pdu_responses,
        };

        savefile::save_file(cache_path, 0, &cache).expect("Save cache");

        log::debug!("Done caching");

        DummyTxRxFut {
            tx: pdu_tx,
            rx: pdu_rx,
            pdu_sends: cache.pdu_sends,
            pdu_responses: cache.pdu_responses,
        }
    } else {
        DummyTxRxFut {
            tx: pdu_tx,
            rx: pdu_rx,
            pdu_sends,
            pdu_responses,
        }
    };

    Ok(task)
}
