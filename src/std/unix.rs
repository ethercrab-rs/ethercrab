//! Items to use when not in `no_std` environments.

use crate::{
    error::Error,
    pdu_loop::{PduRx, PduTx},
};
use core::future::Future;
use embassy_futures::select;
use smoltcp::phy::{Device, Medium, RxToken, TxToken};
use std::io;

/// Spawn a TX and RX task.
pub fn tx_rx_task(
    interface: &str,
    pdu_tx: PduTx<'static>,
    pdu_rx: PduRx<'static>,
) -> Result<impl Future<Output = Result<(), Error>>, std::io::Error> {
    let socket = smoltcp::phy::RawSocket::new(interface, Medium::Ethernet)?;
    let socket_rx = smoltcp::phy::RawSocket::new(interface, Medium::Ethernet)?;

    let mut async_socket = smol::Async::new(socket)?;
    let mut async_socket_rx = smol::Async::new(socket_rx)?;

    let task = async move {
        let tx_task = async {
            while let Some(frame) = pdu_tx.next().await {
                async_socket
                    .write_with_mut(|iface| {
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

                        Ok(())
                    })
                    .await
                    .expect("TX");

                frame.mark_sent();
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

        // select::select3(tx_wake, tx_task, rx_task).await;
        select::select(tx_task, rx_task).await;

        Ok(())
    };

    Ok(task)
}
