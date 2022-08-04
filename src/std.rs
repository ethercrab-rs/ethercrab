use crate::{client::Client, timer_factory::TimerFactory};
use core::{future::Future, task::Poll};
use futures_lite::FutureExt;
use pnet::datalink::{self, DataLinkReceiver, DataLinkSender};
use std::sync::Arc;

pub fn get_tx_rx(
    device: &str,
) -> Result<(Box<dyn DataLinkSender>, Box<dyn DataLinkReceiver>), std::io::Error> {
    let interfaces = datalink::interfaces();

    dbg!(&interfaces);

    let interface = interfaces
        .into_iter()
        .find(|interface| interface.name == device)
        .expect("Could not find interface");

    dbg!(interface.mac);

    let (tx, rx) = match datalink::channel(&interface, Default::default()) {
        Ok(datalink::Channel::Ethernet(tx, rx)) => (tx, rx),
        // FIXME
        Ok(_) => panic!("Unhandled channel type"),
        Err(e) => return Err(e),
    };

    Ok((tx, rx))
}

// TODO: Proper error - there are a couple of unwraps in here
// TODO: Make some sort of split() method to ensure we can only ever have one tx/rx future running
pub fn tx_rx_task<
    'a,
    const MAX_FRAMES: usize,
    const MAX_PDU_DATA: usize,
    const MAX_SLAVES: usize,
    TIMEOUT,
>(
    device: &str,
    client: &'a Arc<Client<MAX_FRAMES, MAX_PDU_DATA, MAX_SLAVES, TIMEOUT>>,
) -> Result<impl Future<Output = ()>, std::io::Error>
where
    TIMEOUT: TimerFactory + Send + 'static,
{
    let client_tx = client.clone();
    let client_rx = client.clone();

    let (mut tx, mut rx) = get_tx_rx(device)?;

    let mut packet_buf = [0u8; 1536];

    // TODO: Unwraps
    let tx_task = futures_lite::future::poll_fn::<(), _>(move |ctx| {
        client_tx
            .pdu_loop
            .send_frames_blocking(ctx.waker(), |pdu| {
                let packet = pdu.to_ethernet_frame(&mut packet_buf).unwrap();

                tx.send_to(packet, None).unwrap().map_err(|_| ())
            })
            .unwrap();

        Poll::Pending
    });

    // TODO: Unwraps
    let rx_task = smol::unblock(move || {
        loop {
            match rx.next() {
                Ok(packet) => {
                    client_rx
                        .pdu_loop
                        .pdu_rx(packet)
                        .map_err(|e| {
                            trace!("Packet len {}", packet.len());

                            e
                        })
                        .expect("RX");
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
