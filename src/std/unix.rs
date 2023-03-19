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
    mut pdu_tx: PduTx<'static>,
    mut pdu_rx: PduRx<'static>,
) -> Result<impl Future<Output = Result<(), Error>>, std::io::Error> {
    let socket = smoltcp::phy::RawSocket::new(interface, Medium::Ethernet)?;
    let socket_rx = smoltcp::phy::RawSocket::new(interface, Medium::Ethernet)?;

    let mut async_socket = smol::Async::new(socket)?;
    let mut async_socket_rx = smol::Async::new(socket_rx)?;

    let mut packet_buf = [0u8; 1536];

    let task = async move {
        let tx_task = async {
            loop {
                let frames = pdu_tx.next().await;

                for frame in frames {
                    frame
                        .send(&mut packet_buf, |frame_bytes| async {
                            async_socket
                                .write_with_mut(|iface| {
                                    // transmit() always returns Some() (at time of writing), so this unwrap
                                    // should optimise out.
                                    let tx_token = iface.transmit().unwrap();

                                    tx_token
                                        .consume(
                                            smoltcp::time::Instant::now(),
                                            frame_bytes.len(),
                                            |buf| {
                                                buf.copy_from_slice(frame_bytes);

                                                Ok(())
                                            },
                                        )
                                        .expect("consume");

                                    Ok(())
                                })
                                .await
                                .map_err(|e| {
                                    log::error!("Send failed: {}", e);

                                    Error::SendFrame
                                })
                        })
                        .await
                        .expect("TX");
                }
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

        select::select(tx_task, rx_task).await;

        Ok(())
    };

    Ok(task)
}
