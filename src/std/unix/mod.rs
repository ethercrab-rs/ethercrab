//! Items to use when not in `no_std` environments.

#[cfg(all(not(target_os = "linux"), unix))]
mod bpf;
#[cfg(target_os = "linux")]
mod linux;

#[cfg(all(not(target_os = "linux"), unix))]
use self::bpf::BpfDevice as RawSocketDesc;
#[cfg(target_os = "linux")]
use self::linux::RawSocketDesc;

use crate::{
    error::Error,
    fmt,
    pdu_loop::{PduRx, PduTx},
};
use async_io::Async;
use core::{future::Future, pin::Pin, task::Poll};
use futures_lite::{AsyncRead, AsyncWrite};
use io_uring::IoUring;
use std::os::fd::AsRawFd;

struct TxRxFut<'a> {
    socket: Async<RawSocketDesc>,
    mtu: usize,
    tx: PduTx<'a>,
    rx: PduRx<'a>,
}

impl Future for TxRxFut<'_> {
    type Output = Result<(), Error>;

    fn poll(mut self: Pin<&mut Self>, ctx: &mut core::task::Context<'_>) -> Poll<Self::Output> {
        let mut buf = vec![0; self.mtu];

        // Re-register waker to make sure this future is polled again
        self.tx.replace_waker(ctx.waker());

        while let Some(frame) = self.tx.next_sendable_frame() {
            let res = frame.send_blocking(&mut buf, |data| {
                match Pin::new(&mut self.socket).poll_write(ctx, data) {
                    Poll::Ready(Ok(bytes_written)) => {
                        if bytes_written != data.len() {
                            fmt::error!("Only wrote {} of {} bytes", bytes_written, data.len());

                            Err(Error::PartialSend {
                                len: data.len(),
                                sent: bytes_written,
                            })
                        } else {
                            Ok(bytes_written)
                        }
                    }

                    Poll::Ready(Err(e)) => {
                        fmt::error!("Send PDU failed: {}", e);

                        Err(Error::SendFrame)
                    }
                    Poll::Pending => Ok(0),
                }
            });

            if let Err(e) = res {
                fmt::error!("Send PDU failed: {}", e);

                return Poll::Ready(Err(e));
            }
        }

        let mut buf = vec![0; self.mtu];

        match Pin::new(&mut self.socket).poll_read(ctx, &mut buf) {
            Poll::Ready(Ok(n)) => {
                fmt::trace!("Poll ready");
                // Wake again in case there are more frames to consume. This is additionally
                // important for macOS as multiple packets may be received for one `poll_read`
                // call, but will only be returned during the _next_ `poll_read`. If this line
                // is removed, PDU response frames are missed, causing timeout errors.
                ctx.waker().wake_by_ref();

                let packet = &buf[0..n];

                if n == 0 {
                    fmt::warn!("Received zero bytes");
                }

                if let Err(e) = self.rx.receive_frame(packet) {
                    fmt::error!("Failed to receive frame: {}", e);

                    return Poll::Ready(Err(Error::ReceiveFrame));
                }
            }
            Poll::Ready(Err(e)) => {
                fmt::error!("Receive PDU failed: {}", e);
            }
            Poll::Pending => (),
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
    };

    Ok(task)
}

/// Create a blocking TX/RX loop using `io_uring`.
#[cfg(target_os = "linux")]
pub async fn tx_rx_task_io_uring<'sto>(
    interface: &str,
    mut pdu_tx: PduTx<'sto>,
    mut pdu_rx: PduRx<'sto>,
) -> Result<(), std::io::Error> {
    use core::{cell::UnsafeCell, future::poll_fn, slice};
    use io_uring::opcode;
    use std::thread::yield_now;

    let mut socket = RawSocketDesc::new(interface)?;

    let mtu = socket.interface_mtu()?;

    fmt::debug!("Opening {} with MTU {}, blocking", interface, mtu);

    // TODO: Use PDU loop entry length? Mul by 2?
    let entries = 8usize;

    let mut ring = IoUring::new(entries as u32)?;

    let mut buffers = {
        let mut buffers = Vec::new();

        for _ in 0..entries {
            buffers.push(UnsafeCell::new(vec![0u8; mtu]));
        }

        buffers
    };

    let mut idx = 0usize;

    // loop {
    //     while let Some(frame) = pdu_tx.next_sendable_frame() {
    //         let mut write_buf = unsafe {
    //             let entry = &mut *buffers[idx % entries].get();

    //             slice::from_raw_parts_mut(entry.as_mut_ptr(), entry.len())
    //         };

    //         frame
    //             .send_blocking(&mut write_buf, |data: &[u8]| {
    //                 let e_send = opcode::Write::new(
    //                     io_uring::types::Fd(socket.as_raw_fd()),
    //                     data.as_ptr(),
    //                     data.len() as _,
    //                 )
    //                 .build()
    //                 .user_data((idx % entries) as u64);

    //                 idx += 1;

    //                 let read_buf = unsafe {
    //                     let entry = &mut *buffers[idx % entries].get();

    //                     slice::from_raw_parts_mut(entry.as_mut_ptr(), entry.len())
    //                 };

    //                 let e_receive = opcode::Read::new(
    //                     io_uring::types::Fd(socket.as_raw_fd()),
    //                     read_buf.as_mut_ptr(),
    //                     read_buf.len() as _,
    //                 )
    //                 .build()
    //                 .user_data((idx % entries) as u64);

    //                 unsafe { ring.submission().push(&e_send) }.expect("Send queue full");
    //                 unsafe { ring.submission().push(&e_receive) }.expect("Send queue full");

    //                 Ok(data.len())
    //             })
    //             .expect("Send blocking");

    //         // TODO: Poll future at least once so it sets its waker

    //         idx += 1;
    //     }

    //     assert_eq!(
    //         ring.submission().cq_overflow(),
    //         false,
    //         "Completion queue overflow 1"
    //     );
    //     assert_eq!(
    //         ring.completion().overflow(),
    //         0,
    //         "Completion queue overflow 2"
    //     );

    //     ring.submit().expect("Submit");

    //     // FIXME: Frame futures need to have been polled at least once to register waker by this
    //     // point. This delay bandaids the shit out of that problem.
    //     // std::thread::sleep(core::time::Duration::from_micros(200));
    //     yield_now();

    //     for recv in ring.completion() {
    //         // TODO: If future doesn't have a waker yet, keep it and retry the wake in the next iteration. Hmm.

    //         // dbg!(&recv, &buffers[recv.user_data() as usize].get_mut()[0..16]);
    //         pdu_rx
    //             .receive_frame(&buffers[recv.user_data() as usize].get_mut())
    //             .expect("Receive frame");
    //     }

    //     yield_now();
    // }

    poll_fn(|ctx| {
        // Re-register waker to make sure this future is polled again
        pdu_tx.replace_waker(ctx.waker());

        let mut sent = 0;

        while let Some(frame) = pdu_tx.next_sendable_frame() {
            let mut write_buf = unsafe {
                let entry = &mut *buffers[idx % entries].get();

                slice::from_raw_parts_mut(entry.as_mut_ptr(), entry.len())
            };

            frame
                .send_blocking(&mut write_buf, |data: &[u8]| {
                    let e_send = opcode::Write::new(
                        io_uring::types::Fd(socket.as_raw_fd()),
                        data.as_ptr(),
                        data.len() as _,
                    )
                    .build()
                    .user_data((idx % entries) as u64);

                    idx += 1;

                    unsafe { ring.submission().push(&e_send) }.expect("Send queue full");

                    Ok(data.len())
                })
                .expect("Send blocking");

            let read_buf = unsafe {
                let entry = &mut *buffers[idx % entries].get();

                slice::from_raw_parts_mut(entry.as_mut_ptr(), entry.len())
            };

            let e_receive = opcode::Read::new(
                io_uring::types::Fd(socket.as_raw_fd()),
                read_buf.as_mut_ptr(),
                read_buf.len() as _,
            )
            .build()
            .user_data((idx % entries) as u64);

            unsafe { ring.submission().push(&e_receive) }.expect("Send queue full");

            idx += 1;
            sent += 1;
        }

        assert_eq!(
            ring.submission().cq_overflow(),
            false,
            "Completion queue overflow 1"
        );
        assert_eq!(
            ring.completion().overflow(),
            0,
            "Completion queue overflow 2"
        );

        fmt::trace!("Sent {} frames", sent);

        // Wait for two responses; one is the packet we just sent which will be ignored, the other
        // is the response from said packet.
        ring.submit_and_wait(sent * 2).expect("Submit");

        fmt::trace!(
            "Submitted, waiting for {} completions",
            ring.completion().len()
        );

        for recv in ring.completion() {
            let foo = buffers[recv.user_data() as usize].get_mut();
            // dbg!(&recv, &buffers[recv.user_data() as usize].get_mut()[0..16]);
            fmt::trace!("Raw frame #{}, addr start {:02x}", foo[0x11], foo[6]);
            pdu_rx
                .receive_frame(&buffers[recv.user_data() as usize].get_mut())
                .expect("Receive frame");
        }

        Poll::Pending
    })
    .await
}

// Unix only
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
