//! Items to use when not in `no_std` environments.

#[cfg(all(not(target_os = "linux"), unix))]
mod bpf;
#[cfg(target_os = "linux")]
mod linux;

#[cfg(all(not(target_os = "linux"), unix))]
use self::bpf::BpfDevice as RawSocketDesc;
#[cfg(target_os = "linux")]
pub(in crate::std) use self::linux::RawSocketDesc;

use crate::{
    error::{Error, PduError},
    fmt,
    pdu_loop::{PduRx, PduTx},
};
use async_io::Async;
use core::{future::Future, pin::Pin, task::Poll, time::Duration};
use std::{
    io::{self, Read, Write},
    thread,
    time::Instant,
};

struct TxRxFut<'a> {
    socket: Async<RawSocketDesc>,
    mtu: usize,
    tx: PduTx<'a>,
    rx: PduRx<'a>,
    in_flight: usize,
}

/// The maximum time `TxRxFut` will busyloop trying to receive a frame that does not yet have a
/// waker.
///
/// If no waker is set after this timeout, `TxRxFut` will continue receiving other frames.
const WAKER_TIMEOUT: Duration = Duration::from_micros(10_000);

impl Future for TxRxFut<'_> {
    type Output = Result<(), Error>;

    fn poll(mut self: Pin<&mut Self>, ctx: &mut core::task::Context<'_>) -> Poll<Self::Output> {
        let mut buf = vec![0; self.mtu + 18];

        // Re-register waker to make sure this future is polled again
        self.tx.replace_waker(ctx.waker());

        while let Some(frame) = self.tx.next_sendable_frame() {
            let res = frame.send_blocking(&mut buf, |data| {
                match unsafe { self.socket.get_mut() }.write(data) {
                    Ok(bytes_written) => {
                        if bytes_written != data.len() {
                            fmt::error!("Only wrote {} of {} bytes", bytes_written, data.len());

                            Err(Error::PartialSend {
                                len: data.len(),
                                sent: bytes_written,
                            })
                        } else {
                            self.in_flight += 1;

                            Ok(bytes_written)
                        }
                    }
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => Ok(0),
                    Err(e) => {
                        fmt::error!("Send PDU failed: {}", e);

                        Err(Error::SendFrame)
                    }
                }
            });

            if let Err(e) = res {
                fmt::error!("Send PDU failed: {}", e);

                return Poll::Ready(Err(e));
            }
        }

        loop {
            let packet = match unsafe { self.socket.get_mut() }.read(&mut buf) {
                Ok(0) => {
                    break;
                }
                Ok(n) => &buf[0..n],
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    break;
                }
                Err(e) => {
                    fmt::error!("Read RX failed: {}", e);

                    return Poll::Ready(Err(Error::ReceiveFrame));
                }
            };

            // if packet.len() > 60 {
            //     dbg!(packet.len());
            // }

            let now = Instant::now();

            // This loop should very rarely iterate more than once. It's here to give the
            // future that sent the frame time to be woken at least once, therefore setting
            // its waker.
            loop {
                match self.rx.receive_frame(packet) {
                    Ok(consumed) => {
                        fmt::trace!(
                            "--> Received frame {:#04x}, {} bytes",
                            packet[0x11],
                            consumed
                        );

                        // assert_eq!(consumed, packet.len());

                        self.in_flight -= 1;

                        break;
                    }

                    // The future behind this frame hasn't been woken yet. We'll queue this
                    // frame for a re-receive so it's not lost.
                    Err(Error::Pdu(PduError::NoWaker)) => {
                        fmt::debug!(
                            "--> No waker for received frame {:#04x} ({} bytes), retrying receive",
                            packet[0x11],
                            packet.len(),
                        );

                        thread::yield_now();
                    }
                    Err(e) => {
                        fmt::error!("Failed to receive frame: {}", e);

                        return Poll::Ready(Err(Error::ReceiveFrame));
                    }
                }

                if now.elapsed() > WAKER_TIMEOUT {
                    fmt::warn!(
                        "--> Timed out waiting for waker for frame {:#04x} ({} us)",
                        packet[0x11],
                        WAKER_TIMEOUT.as_micros()
                    );

                    break;
                }
            }
        }

        // ---

        // let mut rxbuf = Vec::new();

        // while self.in_flight > 0 {
        //     loop {
        //         match unsafe { self.socket.get_mut() }.read(&mut buf) {
        //             Ok(0) => {
        //                 break;
        //             }
        //             Ok(n) => {
        //                 let packet = &buf[0..n];

        //                 rxbuf.extend_from_slice(packet);
        //             }
        //             Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
        //                 break;
        //             }
        //             Err(e) => {
        //                 fmt::error!("Receive PDU failed: {}", e);

        //                 break;
        //             }
        //         }
        //     }

        //     if !rxbuf.is_empty() {
        //         fmt::trace!("RX buffer {} bytes", rxbuf.len());
        //     }

        //     // Consume as much as we can from the received frame(s). Minimum Ethernet frame size is
        //     // 60 bytes
        //     while rxbuf.len() >= 60 {
        //         let packet = rxbuf.as_slice();

        //         let now = Instant::now();

        //         // This loop should very rarely iterate more than once. It's here to give the
        //         // future that sent the frame time to be woken at least once, therefore setting
        //         // its waker.
        //         loop {
        //             match self.rx.receive_frame(packet) {
        //                 Ok(consumed) => {
        //                     fmt::trace!(
        //                         "--> Received frame {:#04x}, {} bytes",
        //                         packet[0x11],
        //                         consumed
        //                     );

        //                     rxbuf = rxbuf.split_off(consumed);

        //                     self.in_flight -= 1;

        //                     break;
        //                 }

        //                 // The future behind this frame hasn't been woken yet. We'll queue this
        //                 // frame for a re-receive so it's not lost.
        //                 Err(Error::Pdu(PduError::NoWaker)) => {
        //                     fmt::debug!(
        //                         "--> No waker for received frame {:#04x} ({} bytes), retrying receive",
        //                         packet[0x11],
        //                         packet.len(),
        //                     );

        //                     thread::yield_now();
        //                 }
        //                 Err(e) => {
        //                     fmt::error!("Failed to receive frame: {}", e);

        //                     return Poll::Ready(Err(Error::ReceiveFrame));
        //                 }
        //             }

        //             if now.elapsed() > WAKER_TIMEOUT {
        //                 fmt::warn!(
        //                     "--> Timed out waiting for waker for frame {:#04x} ({} us)",
        //                     packet[0x11],
        //                     WAKER_TIMEOUT.as_micros()
        //                 );

        //                 break;
        //             }
        //         }
        //     }
        // }

        if self.in_flight > 0 {
            // Wake again in case there are more frames to consume. This is additionally important
            // for macOS as multiple packets may be received for one `poll_read` call, but will only
            // be returned during the _next_ `poll_read`. If this line is removed, PDU response
            // frames are missed, causing timeout errors.
            ctx.waker().wake_by_ref();
        }

        Poll::Pending
    }
}

/// Spawn a TX and RX task.
pub fn tx_rx_task<'sto>(
    interface: &str,
    pdu_tx: PduTx<'sto>,
    #[allow(unused_mut)] mut pdu_rx: PduRx<'sto>,
) -> Result<impl Future<Output = Result<(), Error>> + 'sto, std::io::Error> {
    let mut socket = RawSocketDesc::new(interface)?;

    // macOS forcibly sets the source address to the NIC's MAC, so instead of using `MASTER_ADDR`
    // for filtering returned packets, we must set the address to compare to the NIC MAC.
    #[cfg(all(not(target_os = "linux"), unix))]
    if let Some(mac) = socket.mac().ok().flatten() {
        fmt::debug!("Setting source MAC to {}", mac);

        pdu_rx.set_source_mac(mac);
    }

    let mtu = socket.interface_mtu()?;

    fmt::debug!("Opening {} with MTU {}", interface, mtu);

    let async_socket = async_io::Async::new(socket)?;

    let task = TxRxFut {
        socket: async_socket,
        mtu,
        tx: pdu_tx,
        rx: pdu_rx,
        in_flight: 0,
    };

    Ok(task)
}

// Unix only
#[allow(trivial_numeric_casts)]
fn ifreq_for(name: &str) -> ifreq {
    let mut ifreq = ifreq {
        ifr_name: [0; libc::IF_NAMESIZE],
        ifr_data: 0,
    };
    for (i, byte) in name.as_bytes().iter().enumerate() {
        ifreq.ifr_name[i] = *byte as libc::c_char
    }
    ifreq
}

#[repr(C)]
#[derive(Debug)]
#[allow(non_camel_case_types)]
struct ifreq {
    ifr_name: [libc::c_char; libc::IF_NAMESIZE],
    ifr_data: libc::c_int, /* ifr_ifindex or ifr_mtu */
}
