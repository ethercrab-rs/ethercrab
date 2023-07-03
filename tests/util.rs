//! Utilities to replay Wireshark captures as part of regression/integration tests.

use ethercrab::{error::Error, std::tx_rx_task, PduRx, PduTx};
use pcap_file::pcapng::{Block, PcapNgReader};
use smoltcp::wire::{EthernetAddress, EthernetFrame};
use std::{fs::File, future::Future, pin::Pin, task::Poll};

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

struct DummyTxRxFut<'a> {
    tx: PduTx<'a>,
    rx: PduRx<'a>,

    capture_file: PcapNgReader<File>,

    /// Packet number from Wireshark capture.
    packet_number: usize,
}

impl<'a> DummyTxRxFut<'a> {
    const MASTER_ADDR: EthernetAddress = EthernetAddress([0x10, 0x10, 0x10, 0x10, 0x10, 0x10]);
    const REPLY_ADDR: EthernetAddress = EthernetAddress([0x12, 0x10, 0x10, 0x10, 0x10, 0x10]);

    fn next_line(&mut self) -> EthernetFrame<Vec<u8>> {
        while let Some(block) = self.capture_file.next_block() {
            self.packet_number += 1;

            // Check if there is no error
            let block = block.expect("Block error");

            let raw = match block {
                Block::EnhancedPacket(block) => {
                    let buf = block.data.to_owned();

                    let buf2 = buf.iter().copied().collect::<Vec<_>>();

                    EthernetFrame::new_checked(buf2).expect("Failed to parse block")
                }
                Block::InterfaceDescription(_) => continue,
                other => panic!(
                    "Frame {} is not correct type: {:?}",
                    self.packet_number, other
                ),
            };

            if raw.src_addr() != Self::MASTER_ADDR && raw.src_addr() != Self::REPLY_ADDR {
                panic!(
                    "Frame {} does not have EtherCAT address (has {:?} instead)",
                    self.packet_number,
                    raw.src_addr()
                );
            }

            return raw;
        }

        unreachable!();
    }

    fn next_line_is_send(&mut self) -> EthernetFrame<Vec<u8>> {
        let next = self.next_line();

        assert_eq!(next.src_addr(), Self::MASTER_ADDR);

        next
    }

    fn next_line_is_reply(&mut self) -> EthernetFrame<Vec<u8>> {
        let next = self.next_line();

        assert_eq!(next.src_addr(), Self::REPLY_ADDR);

        next
    }
}

impl Future for DummyTxRxFut<'_> {
    type Output = Result<(), Error>;

    fn poll(mut self: Pin<&mut Self>, ctx: &mut core::task::Context<'_>) -> Poll<Self::Output> {
        let mut waker = self.tx.lock_waker();

        let mut buf = [0u8; 1536];

        while let Some(frame) = self.tx.next_sendable_frame() {
            let expected = self.next_line_is_send();

            frame
                .send_blocking(&mut buf, |got| {
                    assert_eq!(expected.as_ref(), got, "TX line {}", self.packet_number);

                    Ok(())
                })
                .expect("Failed to send");

            // ---

            let expected = self.next_line_is_reply();

            self.rx.receive_frame(expected.as_ref()).expect("Frame RX")
        }

        waker.replace(ctx.waker().clone());

        Poll::Pending
    }
}

/// Spawn a TX and RX task.
pub fn dummy_tx_rx_task(
    capture_file_path: &str,
    pdu_tx: PduTx<'static>,
    pdu_rx: PduRx<'static>,
) -> Result<impl Future<Output = Result<(), Error>>, std::io::Error> {
    let file_in = File::open(capture_file_path).expect("Error opening file");
    let pcapng_reader = PcapNgReader::new(file_in).expect("Failed to init PCAP reader");

    let task = DummyTxRxFut {
        tx: pdu_tx,
        rx: pdu_rx,
        capture_file: pcapng_reader,
        // Indexed from 1 in the Wireshark UI
        packet_number: 1,
    };

    Ok(task)
}
