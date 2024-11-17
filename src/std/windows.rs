//! Items to use when not in `no_std` environments.

use crate::{
    error::{Error, PduError},
    ethernet::EthernetFrame,
    fmt,
    pdu_loop::{PduRx, PduTx},
    std::ParkSignal,
    ReceiveAction,
};
use pnet_datalink::{self, channel, Channel, DataLinkReceiver, DataLinkSender};
use std::io;
use std::{future::Future, sync::Arc, task::Waker, thread, time::SystemTime};

/// Get a TX/RX pair.
fn get_tx_rx(
    device: &str,
) -> Result<(Box<dyn DataLinkSender>, Box<dyn DataLinkReceiver>), std::io::Error> {
    let interfaces = pnet_datalink::interfaces();

    let interface = match interfaces.iter().find(|interface| interface.name == device) {
        Some(interface) => interface,
        None => {
            fmt::error!("Could not find interface {device}");

            fmt::error!("Available interfaces:");

            for interface in interfaces.iter() {
                fmt::error!("-> {} {}", interface.name, interface.description);
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
#[deprecated = "use `tx_rx_task_blocking` instead"]
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
                        fmt::trace!("Send frame {:#04x}, {} bytes", idx, frame_bytes.len());

                        tx.send_to(frame_bytes, None)
                            .ok_or(Error::SendFrame)?
                            .map_err(|e| {
                                fmt::error!("Failed to send packet: {}", e);

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
                                    fmt::warn!("{} existing frame bytes", frame_buf.len());
                                }

                                frame_buf.extend_from_slice(ethernet_frame);
                            }
                            // Truncated frame - try adding them together
                            Err(_) => {
                                fmt::warn!("Truncated frame: len {}", ethernet_frame.len());

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
                        fmt::error!("An error occurred while receiving frame bytes: {}", e);

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
                    fmt::error!(
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

/// Create a blocking task that waits for PDUs to send, and receives PDU responses.
pub fn tx_rx_task_blocking<'sto>(
    device: &str,
    mut pdu_tx: PduTx<'sto>,
    mut pdu_rx: PduRx<'sto>,
) -> Result<(), std::io::Error> {
    let signal = Arc::new(ParkSignal::new());
    let waker = Waker::from(Arc::clone(&signal));

    let mut cap = pcap::Capture::from_device(device)
        .expect("Device")
        .immediate_mode(true)
        .open()
        .expect("Open device")
        .setnonblock()
        .expect("Can't set non-blocking");

    // 1MB send queue.
    let mut sq = pcap::sendqueue::SendQueue::new(1024 * 1024).expect("Failed to create send queue");

    let mut in_flight = 0usize;

    loop {
        fmt::trace!("Begin TX/RX iteration");

        pdu_tx.replace_waker(&waker);

        let mut sent_this_iter = 0usize;

        while let Some(frame) = pdu_tx.next_sendable_frame() {
            let idx = frame.index();

            frame
                .send_blocking(|frame_bytes| {
                    fmt::trace!("Send frame {:#04x}, {} bytes", idx, frame_bytes.len());

                    // Add 256 bytes of L2 payload
                    sq.queue(None, frame_bytes).expect("Enqueue");

                    Ok(frame_bytes.len())
                })
                .map_err(std::io::Error::other)?;

            sent_this_iter += 1;
        }

        // Send any queued packets
        if sent_this_iter > 0 {
            fmt::trace!("Send {} enqueued frames", sent_this_iter);

            // SendSync::Off = transmit with no delay between packets
            sq.transmit(&mut cap, pcap::sendqueue::SendSync::Off)
                .expect("Transmit");

            in_flight += sent_this_iter;
        }

        if in_flight > 0 {
            debug_assert!(cap.is_nonblock(), "Must be in non-blocking mode");

            fmt::trace!("{} frames are in flight", in_flight);

            // Receive any in-flight frames
            loop {
                match cap.next_packet() {
                    // NOTE: We receive our own sent frames. `receive_frame` will make sure they're
                    // ignored.
                    Ok(packet) => {
                        let frame_buf = packet.data;

                        let frame_index = frame_buf
                            .get(0x11)
                            .ok_or_else(|| io::Error::other(Error::Internal))?;

                        let res = loop {
                            match pdu_rx.receive_frame(&frame_buf) {
                                Ok(res) => break res,
                                Err(Error::Pdu(PduError::NoWaker)) => {
                                    fmt::trace!(
                                        "No waker for received frame {:#04x}, retrying receive",
                                        frame_index
                                    );

                                    thread::yield_now();
                                }
                                Err(e) => return Err(io::Error::other(e)),
                            }
                        };

                        fmt::trace!(
                            "Received and {:?} frame {:#04x} ({} bytes)",
                            res,
                            frame_index,
                            packet.header.len
                        );

                        if res == ReceiveAction::Processed {
                            in_flight = in_flight
                                .checked_sub(1)
                                .expect("More frames processed than in flight");
                        }
                    }
                    Err(pcap::Error::NoMorePackets) => {
                        // Nothing to read yet

                        break;
                    }
                    Err(pcap::Error::TimeoutExpired) => {
                        // Timeouts are instant as we're in non-blocking mode (I think), so we just
                        // ignore them (we're spinlooping while packets are in flight essentially).

                        break;
                    }
                    Err(e) => {
                        fmt::error!("Packet receive failed: {}", e);

                        // Quit the TX/RX loop - we failed somewhere
                        // TODO: Allow this to be configured so we ignore RX failures
                        return Err(io::Error::other(e));
                    }
                }
            }
        }
        // No frames in flight. Wait to be woken again by something sending a frame
        else {
            fmt::trace!("No frames in flight, waiting to be woken with new frames to send");

            signal.wait();
        }
    }

    #[allow(unreachable_code)]
    Ok(())
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
