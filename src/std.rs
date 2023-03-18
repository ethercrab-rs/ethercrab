//! Items to use when not in `no_std` environments.

use crate::pdu_loop::{PduRx, PduTx};
use core::{future::Future, task::Poll};
use embassy_futures::select;
use pnet::datalink::{self, DataLinkReceiver, DataLinkSender};
use smoltcp::{
    phy::{Device, Medium, RxToken, TxToken},
    wire::EthernetFrame,
};
use std::{io, sync::Arc};

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

    dbg!(interface);

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
    pdu_tx: PduTx<'static>,
    pdu_rx: PduRx<'static>,
) -> Result<impl Future<Output = embassy_futures::select::Either<(), ()>>, std::io::Error> {
    let (mut tx, mut rx) = get_tx_rx(device)?;

    let mut packet_buf = [0u8; 1536];

    // TODO: Unwraps
    let tx_task = core::future::poll_fn::<(), _>(move |ctx| {
        pdu_tx
            .send_frames_blocking(ctx.waker(), &mut packet_buf, |frame| {
                tx.send_to(frame, None).unwrap().map_err(|e| {
                    log::error!("Failed to send packet: {e}");
                })
            })
            .unwrap();

        Poll::Pending
    });

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

    Ok(select::select(tx_task, rx_task))
}

/// Spawn a TX and RX task.
// TODO: Return a result
pub fn tx_rx_task_new(
    interface: String,
    pdu_tx: PduTx<'static>,
    pdu_rx: PduRx<'static>,
) -> impl Future<Output = ()> {
    let socket = smoltcp::phy::RawSocket::new(&interface, Medium::Ethernet).unwrap();
    let socket_rx = smoltcp::phy::RawSocket::new(&interface, Medium::Ethernet).unwrap();

    let mut async_socket = smol::Async::new(socket).unwrap();
    let mut async_socket_rx = smol::Async::new(socket_rx).unwrap();

    let notify = Arc::new(async_notify::Notify::new());
    let notify2 = notify.clone();

    let task = async move {
        let tx_wake = core::future::poll_fn(|ctx| {
            pdu_tx.set_waker(ctx.waker().clone());

            notify.notify();

            Poll::<()>::Pending
        });

        let tx_task = async {
            loop {
                notify2.notified().await;

                async_socket
                    .write_with_mut(|iface| {
                        if let Some(frame) = pdu_tx.next_sendable_frame() {
                            // transmit() always returns Some() (at time of writing), so this unwrap
                            // should optimise out.
                            let tx_token = iface.transmit().unwrap();

                            tx_token
                                .consume(
                                    smoltcp::time::Instant::now(),
                                    frame.ethernet_frame_len(),
                                    |buf| {
                                        frame.write_ethernet_packet(buf).expect("write frame");

                                        Ok(())
                                    },
                                )
                                .expect("consume");

                            frame.mark_sent();

                            // There might be more frames to send. WouldBlock signals that this
                            // closure should be called again.
                            Err::<(), _>(io::Error::from(io::ErrorKind::WouldBlock))
                        } else {
                            Ok(())
                        }
                    })
                    .await
                    .expect("TX");
            }
        };

        let rx_task = async {
            async_socket_rx
                .read_with_mut(|iface| {
                    if let Some((receiver, _)) = iface.receive() {
                        receiver
                            .consume(smoltcp::time::Instant::now(), |frame_buf| {
                                pdu_rx.receive_frame(&frame_buf).map_err(|e| {
                                    log::error!("Failed to receive packet: {}", e);

                                    smoltcp::Error::Malformed
                                })
                            })
                            .expect("RX packet");
                    }

                    Err::<(), _>(io::Error::from(io::ErrorKind::WouldBlock))
                })
                .await
                .expect("Read loop");
        };

        select::select3(tx_wake, tx_task, rx_task).await;
    };

    task
}
