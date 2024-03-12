//! Items to use when not in `no_std` environments.

use crate::{
    error::Error,
    pdu_loop::{PduRx, PduTx},
};
use core::future::Future;
use pnet_datalink::{self, channel, Channel, DataLinkReceiver, DataLinkSender};
use smoltcp::wire::EthernetFrame;
use std::{thread, time::SystemTime};

/// Get a TX/RX pair.
fn get_tx_rx(
    device: &str,
) -> Result<(Box<dyn DataLinkSender>, Box<dyn DataLinkReceiver>), std::io::Error> {
    let interfaces = pnet_datalink::interfaces();

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

    let config = pnet_datalink::Config {
        write_buffer_size: 16384,
        read_buffer_size: 16384,
        ..Default::default()
    };

    let (tx, rx) = match channel(interface, config) {
        Ok(Channel::Ethernet(tx, rx)) => (tx, rx),
        Ok(_) => panic!("Unhandled channel type"),
        Err(e) => return Err(e),
    };

    Ok((tx, rx))
}

/// Create a task that waits for PDUs to send, and receives PDU responses.
pub fn tx_rx_task<'sto>(
    device: &str,
    mut pdu_tx: PduTx<'sto>,
    mut pdu_rx: PduRx<'sto>,
) -> Result<impl Future<Output = Result<(), Error>> + 'sto, std::io::Error> {
    let (mut tx, mut rx) = get_tx_rx(device)?;

    let task = async move {
        let tx_task = async {
            loop {
                while let Some(frame) = pdu_tx.next_sendable_frame() {
                    let idx = frame.index();

                    frame.send_blocking(|frame_bytes| {
                        log::trace!("Send frame {:#04x}, {} bytes", idx, frame_bytes.len());

                        tx.send_to(frame_bytes, None)
                            .ok_or(Error::SendFrame)?
                            .map_err(|e| {
                                log::error!("Failed to send packet: {}", e);

                                Error::SendFrame
                            })?;

                        Ok(frame_bytes.len())
                    })?;
                }

                futures_lite::future::yield_now().await;
            }

            #[allow(unreachable_code)]
            Ok::<(), Error>(())
        };

        let (receive_frame_tx, receive_frame_rx) =
            async_channel::unbounded::<Result<Vec<u8>, Error>>();

        thread::spawn(move || {
            let mut frame_buf: Vec<u8> = Vec::new();

            loop {
                match rx.next() {
                    Ok(ethernet_frame) => {
                        match EthernetFrame::new_unchecked(ethernet_frame).check_len() {
                            // We got a full frame
                            Ok(_) => {
                                if !frame_buf.is_empty() {
                                    log::warn!("{} existing frame bytes", frame_buf.len());
                                }

                                frame_buf.extend_from_slice(ethernet_frame);
                            }
                            // Truncated frame - try adding them together
                            Err(_) => {
                                log::warn!("Truncated frame: len {}", ethernet_frame.len());

                                frame_buf.extend_from_slice(ethernet_frame);

                                continue;
                            }
                        };

                        receive_frame_tx
                            .send_blocking(Ok(frame_buf.clone()))
                            .expect("Channel full or closed");

                        frame_buf.truncate(0);
                    }
                    Err(e) => {
                        log::error!("An error occurred while receiving frame bytes: {}", e);

                        receive_frame_tx
                            .send_blocking(Err(Error::ReceiveFrame))
                            .ok();

                        break;
                    }
                }

                std::thread::yield_now();
            }
        });

        let rx_task = async {
            while let Ok(frame_buf) = receive_frame_rx.recv().await {
                let frame_buf = frame_buf?;

                pdu_rx.receive_frame(&frame_buf).map_err(|e| {
                    log::error!(
                        "Failed to parse received frame: {} (len {} bytes)",
                        e,
                        frame_buf.len()
                    );

                    e
                })?;
            }

            Result::<(), Error>::Ok(())
        };

        futures_lite::future::try_zip(tx_task, rx_task)
            .await
            .map(|_| ())
    };

    Ok(task)
}

/// Get the current time in nanoseconds from the EtherCAT epoch, 2000-01-01.
///
/// Note that on Windows this clock is not monotonic.
pub fn ethercat_now() -> u64 {
    let t = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64;

    // EtherCAT epoch is 2000-01-01
    t.saturating_sub(946684800)
}
