use crate::{client::Client, timer_factory::TimerFactory};
use core::{future::Future, task::Poll};
use embassy_futures::select;
use pnet::datalink::{self, DataLinkReceiver, DataLinkSender};
use smoltcp::wire::EthernetFrame;
use std::sync::Arc;

pub fn get_tx_rx(
    device: &str,
) -> Result<(Box<dyn DataLinkSender>, Box<dyn DataLinkReceiver>), std::io::Error> {
    let interfaces = datalink::interfaces();

    // dbg!(&interfaces);

    let interface = interfaces
        .into_iter()
        .find(|interface| interface.name == device)
        .expect("Could not find interface");

    // dbg!(interface.mac);

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
// TODO: Make some sort of split() method to ensure we can only ever have one tx/rx future running
pub fn tx_rx_task<TIMEOUT>(
    device: &str,
    client: &Arc<Client<'static, TIMEOUT>>,
) -> Result<impl Future<Output = embassy_futures::select::Either<(), ()>>, std::io::Error>
where
    TIMEOUT: TimerFactory + Send + 'static,
{
    let client_tx = client.clone();
    let client_rx = client.clone();

    let (mut tx, mut rx) = get_tx_rx(device)?;

    let mut packet_buf = [0u8; 1536];

    // TODO: Unwraps
    let tx_task = core::future::poll_fn::<(), _>(move |ctx| {
        client_tx
            .pdu_loop
            .send_frames_blocking(ctx.waker(), |frame, data| {
                let packet = frame
                    .write_ethernet_packet(&mut packet_buf, data)
                    .expect("Write Ethernet frame");

                tx.send_to(packet, None).unwrap().map_err(|e| {
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

                    client_rx
                        .pdu_loop
                        .pdu_rx(&frame_buf)
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
