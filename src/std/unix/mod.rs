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
use smallvec::SmallVec;
use std::{
    os::fd::AsRawFd,
    sync::Arc,
    task::Wake,
    thread::{self, Thread},
};

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

// TODO: Linux-only
struct ParkSignal {
    current_thread: Thread,
}

impl ParkSignal {
    fn new() -> Self {
        Self {
            current_thread: thread::current(),
        }
    }

    fn wait(&self) {
        thread::park();
    }
}

impl Wake for ParkSignal {
    fn wake(self: Arc<Self>) {
        self.current_thread.unpark()
    }
}

struct Retry {
    retry_count: usize,
    index: u8,
    frame: SmallVec<[u8; 1518]>,
}

// TODO: Linux-only
/// Create a blocking TX/RX loop using `io_uring`.
///
/// This function is only available on `linux` targets as it requires `io_uring` support. Older
/// kernels may not support `io_uring`.
#[cfg(target_os = "linux")]
pub fn tx_rx_task_io_uring<'sto>(
    interface: &str,
    mut pdu_tx: PduTx<'sto>,
    mut pdu_rx: PduRx<'sto>,
) -> Result<(), std::io::Error> {
    use core::task::Waker;
    use heapless::{Entry, FnvIndexMap};
    use io_uring::opcode;
    use smallvec::smallvec;
    use std::{collections::VecDeque, io, time::Instant};

    use crate::error::PduError;

    let mut socket = RawSocketDesc::new(interface)?;

    let mtu = socket.interface_mtu()?;

    fmt::debug!(
        "Opening {} with MTU {}, blocking, using io_uring",
        interface,
        mtu
    );

    // MTU is payload size. We need to add the layer 2 header which is 18 bytes.
    let mtu = mtu + 18;

    const ENTRIES: usize = 256;

    // SAFETY: Max entries is 256 because `PduStorage::N` is checked to be in 0..u8::MAX, and will
    // eventually be a `u8` once const generics get there.
    let mut bufs = FnvIndexMap::<u8, SmallVec<[u8; 1518]>, ENTRIES>::new();

    // Race condition: sometimes a response can be received before the original future has been
    // polled, therefore has no waker. This is bad but reasonably rare. To mitigate (bandaid...)
    // this problem, we'll add a retry queue that will attempt to re-receive a frame in the hopes
    // that the future has been polled at least once by then, and its waker registered.
    let mut retries = VecDeque::<Retry>::with_capacity(32);

    let mut ring = IoUring::new(ENTRIES as u32)?;

    let mut high_water_mark = 0;
    let mut retries_high_water_mark = 0;

    let signal = Arc::new(ParkSignal::new());
    let waker = Waker::from(Arc::clone(&signal));

    loop {
        pdu_tx.replace_waker(&waker);

        while let Some(mut retry) = retries.pop_front() {
            match pdu_rx.receive_frame(&retry.frame) {
                Ok(_) => (),
                Err(Error::Pdu(PduError::NoWaker)) => {
                    // If this happens too much at startup, there's a chance the TX/RX thread is
                    // taking too long to start. Adding a delay between the TX/RX thread spawn and
                    // the rest of the app may help.
                    fmt::trace!(
                        "No waker for frame #{} receive retry attempt {}, requeueing to try again later",
                        retry.index,
                        retry.retry_count
                    );

                    retry.retry_count += 1;

                    retries.push_back(retry);

                    retries_high_water_mark = retries_high_water_mark.max(retries.len());
                }
                Err(e) => {
                    fmt::error!("Receive frame #{} retry failed: {}", retry.index, e);

                    return Err(io::Error::other(e));
                }
            }
        }

        let mut sent = 0;

        while let Some(frame) = pdu_tx.next_sendable_frame() {
            let idx = frame.index();

            let mut buf = match bufs.entry(idx) {
                Entry::Occupied(_) => {
                    fmt::error!(
                        "io_uring frame slot for index #{} is already in flight",
                        idx
                    );

                    return Err(io::Error::other(Error::SendFrame));
                }
                Entry::Vacant(entry) => entry.insert(smallvec![0; mtu]).map_err(|_| {
                    fmt::error!("failed to insert new frame buffer");

                    io::Error::other(Error::SendFrame)
                }),
            }?;

            frame
                .send_blocking(&mut buf, |data: &[u8]| {
                    let e_send = opcode::Write::new(
                        io_uring::types::Fd(socket.as_raw_fd()),
                        data.as_ptr(),
                        data.len() as _,
                    )
                    .build()
                    // We want to ignore sent frames in the completion queue, so we'll set a
                    // sentinel value here.
                    .user_data(u64::MAX);

                    unsafe { ring.submission().push(&e_send) }.expect("Send queue full");

                    Ok(data.len())
                })
                .expect("Send blocking");

            // Receive back into the same buffer. This should be safe because we can only receive
            // once we've sent the packet.
            let e_receive = opcode::Read::new(
                io_uring::types::Fd(socket.as_raw_fd()),
                buf.as_mut_ptr(),
                buf.len() as _,
            )
            .build()
            .user_data(u64::from(idx));

            high_water_mark = high_water_mark.max(bufs.len());

            unsafe { ring.submission().push(&e_receive) }.expect("Send queue full");

            sent += 1;
        }

        // TODO: Collect these metrics for later gathering instead of just asserting
        // assert_eq!(ring.completion().overflow(), 0);
        // assert_eq!(ring.completion().is_full(), false);
        // assert_eq!(ring.submission().cq_overflow(), false);
        // assert_eq!(ring.submission().dropped(), 0);

        ring.submission().sync();
        ring.submit_and_wait(sent * 2)?;

        fmt::trace!(
            "Submitted, waiting for {} completions",
            ring.completion().len()
        );

        for recv in ring.completion() {
            let index = recv.user_data();

            // Marker flag for a sent frame. We only want receiving frames, so skip this one.
            if index == u64::MAX {
                continue;
            }

            // NOTE: `as` truncates, but the original data was a `u8` anyway.
            let index = index as u8;

            if let Some(frame) = bufs.remove(&index) {
                // dbg!(&recv, &buffers[recv.user_data() as usize].get_mut()[0..16]);
                fmt::trace!(
                    "Raw frame #{:02} buffer idx {} {}",
                    frame[0x11],
                    recv.user_data() as u8,
                    if frame[6] == 0x10 { "---->" } else { "<--" }
                );

                match pdu_rx.receive_frame(&frame) {
                    Ok(_) => (),
                    Err(Error::Pdu(PduError::NoWaker)) => {
                        fmt::debug!(
                            "No waker for received frame #{}, retrying receive later",
                            index
                        );

                        retries.push_back(Retry {
                            retry_count: 1,
                            index,
                            frame,
                        });

                        retries_high_water_mark = retries_high_water_mark.max(retries.len());
                    }
                    Err(e) => return Err(io::Error::other(e)),
                }
            } else {
                fmt::warn!("Tried to receive frame #{} more than once", index);
            }
        }

        // Flush completed entries
        ring.completion().sync();

        if bufs.is_empty() && retries.is_empty() {
            fmt::trace!("No frames in flight, waiting to be woken with new frames to send");

            // Doesn't do anything for weird "blinking" behaviour
            // std::thread::sleep(Duration::from_micros(10));

            let start = Instant::now();

            // This must be after the send packet code as there can be a (safe!) race condition on
            // startup where the TX waker hasn't been registered yet, so when a future from another
            // thread tries to send its frame, it has no waker, so we just end up waiting forever.
            //
            // If this wait() is down here, we get at least one loop where any queued packets can be
            // sent.
            signal.wait();

            fmt::trace!("--> Waited for {} ns", start.elapsed().as_nanos());
        }
    }
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
