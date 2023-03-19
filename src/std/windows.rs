//! Items to use when not in `no_std` environments.

use crate::{
    error::Error,
    pdu_loop::{PduRx, PduTx},
};
use core::future::Future;
use embassy_futures::select;
use pnet::datalink::{self, DataLinkReceiver, DataLinkSender};
use smoltcp::wire::EthernetFrame;

/// Get a TX/RX pair.
fn get_tx_rx(
    device: &str,
) -> Result<(Box<dyn DataLinkSender>, Box<dyn DataLinkReceiver>), std::io::Error> {
    let interfaces = datalink::interfaces();

    let interface = match interfaces.iter().find(|interface| interface.name == device) {
        Some(interface) => interface,
        None => {
            log::error!("Could not find interface {device}");

            log::error!("Available interfaces:");

            for interface in interfaces.iter() {
                log::error!("-> {} {}", interface.name, interface.description);
            }

            panic!();
        }
    };

    let config = pnet::datalink::Config {
        write_buffer_size: 16384,
        read_buffer_size: 16384,
        ..Default::default()
    };

    let (tx, rx) = match datalink::channel(&interface, config) {
        Ok(datalink::Channel::Ethernet(tx, rx)) => (tx, rx),
        Ok(_) => panic!("Unhandled channel type"),
        Err(e) => return Err(e),
    };

    Ok((tx, rx))
}

// TODO: Proper error - there are a couple of unwraps in here
/// Create a task that waits for PDUs to send, and receives PDU responses.
pub fn tx_rx_task(
    device: &str,
    mut pdu_tx: PduTx<'static>,
    mut pdu_rx: PduRx<'static>,
) -> Result<impl Future<Output = Result<(), Error>>, std::io::Error> {
    let (mut tx, mut rx) = get_tx_rx(device)?;

    let mut packet_buf = [0u8; 1536];

    let task = async move {
        // TODO: Unwraps
        let tx_task = async {
            loop {
                let frames = pdu_tx.next().await;

                for frame in frames {
                    frame
                        .send(&mut packet_buf, |frame_bytes| async {
                            tx.send_to(frame_bytes, None)
                                .unwrap()
                                .map_err(|e| {
                                    log::error!("Failed to send packet: {e}");
                                })
                                .expect("TX");

                            Ok(())
                        })
                        .await
                        .expect("TX");
                }
            }
        };

        // TODO: Unwraps
        let rx_task = smol::unblock(move || {
            let mut frame_buf: Vec<u8> = Vec::new();

            loop {
                match rx.next() {
                    Ok(ethernet_frame) => {
                        match EthernetFrame::new_checked(ethernet_frame) {
                            // We got a full frame
                            Ok(_) => {
                                if !frame_buf.is_empty() {
                                    log::warn!("{} existing frame bytes", frame_buf.len());
                                }

                                frame_buf.extend_from_slice(ethernet_frame);
                            }
                            // Truncated frame - try adding them together
                            Err(smoltcp::Error::Truncated) => {
                                log::warn!("Truncated frame: len {}", ethernet_frame.len());

                                frame_buf.extend_from_slice(ethernet_frame);

                                continue;
                            }
                            Err(e) => panic!("RX pre: {e}"),
                        };

                        pdu_rx
                            .receive_frame(&frame_buf)
                            .map_err(|e| {
                                dbg!(frame_buf.len());

                                e
                            })
                            .expect("RX");

                        frame_buf.truncate(0);
                    }
                    Err(e) => {
                        // If an error occurs, we can handle it here
                        panic!("An error occurred while reading: {e}");
                    }
                }
            }
        });

        select::select(tx_task, rx_task).await;

        Ok(())
    };

    Ok(task)
}
