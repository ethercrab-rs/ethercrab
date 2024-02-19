use crate::{
    error::{Error, PduError},
    fmt,
    std::unix::RawSocketDesc,
    PduRx, PduTx,
};
use core::{mem::MaybeUninit, task::Waker};
use io_uring::{opcode, IoUring};
use smallvec::{smallvec, SmallVec};
use std::{
    collections::VecDeque,
    io,
    os::fd::AsRawFd,
    sync::Arc,
    task::Wake,
    thread::{self, Thread},
    time::Instant,
};

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

    // fn wait_timeout(&self, timeout: Duration) {
    //     thread::park_timeout(timeout)
    // }
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

/// Use the upper bit of a u64 to mark whether a frame is a write (`1`) or a read (`0`).
const WRITE_MASK: u64 = 1 << 63;

/// Create a blocking TX/RX loop using `io_uring`.
///
/// This function is only available on `linux` targets as it requires `io_uring` support. Older
/// kernels may not support `io_uring`.
pub fn tx_rx_task_io_uring<'sto>(
    interface: &str,
    mut pdu_tx: PduTx<'sto>,
    mut pdu_rx: PduRx<'sto>,
) -> Result<(), std::io::Error> {
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
    // eventually be a `u8` once const generics get there. Twice as much space is reserved as each
    // frame requires a send _and_ receive buffer.
    //
    // This data MUST NOT MOVE or be reordered once created as io_uring holds pointers into it.
    let mut bufs: slab::Slab<(io_uring::squeue::Entry, SmallVec<[u8; 1518]>)> =
        slab::Slab::with_capacity(ENTRIES * 2);

    // Race condition: sometimes a response can be received before the original future has been
    // polled, therefore has no waker. This is bad but reasonably rare. To mitigate (bandaid...)
    // this problem, we'll add a retry queue that will attempt to re-receive a frame in the hopes
    // that the future has been polled at least once by then, and its waker registered.
    let mut retries: VecDeque<Retry> = VecDeque::<Retry>::with_capacity(32);

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

            let tx_b = bufs.vacant_entry();
            let tx_key = tx_b.key();
            let (tx_entry, tx_buf) = tx_b.insert((
                unsafe { MaybeUninit::zeroed().assume_init() },
                smallvec![0; mtu],
            ));

            frame
                .send_blocking(tx_buf, |data: &[u8]| {
                    *tx_entry = opcode::Write::new(
                        io_uring::types::Fd(socket.as_raw_fd()),
                        data.as_ptr(),
                        data.len() as _,
                    )
                    .build()
                    // Distinguish sent frames from received frames by using the upper bit of
                    // the user data as a flag.
                    .user_data(tx_key as u64 | WRITE_MASK);

                    while unsafe { ring.submission().push(&tx_entry).is_err() } {
                        // If the submission queue is full, flush it to the kernel
                        ring.submit().expect("Internal error, failed to submit ops");
                    }

                    sent += 1;

                    Ok(data.len())
                })
                .expect("Send blocking");

            let rx_b = bufs.vacant_entry();
            let rx_key = rx_b.key();
            let (rx_entry, rx_buf) = rx_b.insert((
                unsafe { MaybeUninit::zeroed().assume_init() },
                smallvec![0; mtu],
            ));

            *rx_entry = opcode::Read::new(
                io_uring::types::Fd(socket.as_raw_fd()),
                rx_buf.as_mut_ptr() as _,
                rx_buf.len() as _,
            )
            .build()
            .user_data(rx_key as u64);

            fmt::trace!(
                "Insert frame TX #{}, key {}, RX key {}",
                idx,
                tx_key,
                rx_key
            );

            while unsafe { ring.submission().push(&rx_entry).is_err() } {
                // If the submission queue is full, flush it to the kernel
                ring.submit().expect("Internal error, failed to submit ops");
            }

            high_water_mark = high_water_mark.max(bufs.len());
        }

        // TODO: Collect these metrics for later gathering instead of just asserting
        // assert_eq!(ring.completion().overflow(), 0);
        // assert_eq!(ring.completion().is_full(), false);
        // assert_eq!(ring.submission().cq_overflow(), false);
        // assert_eq!(ring.submission().dropped(), 0);

        ring.submission().sync();

        let now = Instant::now();

        if sent > 0 {
            ring.submit_and_wait(sent * 2)?;
        }

        fmt::trace!(
            "Submitted, waited for {} completions for {} us",
            ring.completion().len(),
            now.elapsed().as_micros(),
        );

        // SAFETY: We must never call `completion_shared` or `completion` inside this loop.
        for recv in unsafe { ring.completion_shared() } {
            if recv.result() < 0 && recv.result() != -libc::EWOULDBLOCK {
                return Err(io::Error::last_os_error());
            }

            let key = recv.user_data();

            fmt::trace!(
                "Got a frame by key {} -> {} {}",
                key,
                key & !WRITE_MASK,
                if key & WRITE_MASK == WRITE_MASK {
                    "---->"
                } else {
                    "<--"
                }
            );

            // If upper bit is set, this was a write that is now complete. We can remove its buffer
            // from the slab allocator.
            if key & WRITE_MASK == WRITE_MASK {
                let key = key & !WRITE_MASK;

                // Clear send buffer grant as it's been sent over the network
                bufs.remove(key as usize);

                continue;
            }

            // Original read did not succeed. Requeue read so we can try again.
            if recv.result() == -libc::EWOULDBLOCK {
                fmt::trace!("Frame key {} would block. Queuing for retry", key);

                let (rx_entry, _buf) = bufs.get(key as usize).expect("Could not get retry entry");

                // SAFETY: `submission_shared` must not be held at the same time this one is
                while unsafe { ring.submission_shared().push(&rx_entry).is_err() } {
                    // If the submission queue is full, flush it to the kernel
                    ring.submit().expect("Internal error, failed to submit ops");
                }
            } else if let Some((_entry, frame)) = bufs.try_remove(key as usize) {
                let frame_index = frame[0x11];

                fmt::trace!(
                    "Raw frame #{} result {} buffer key {}",
                    frame_index,
                    recv.result(),
                    key,
                );

                match pdu_rx.receive_frame(&frame) {
                    Ok(_) => (),
                    Err(Error::Pdu(PduError::NoWaker)) => {
                        fmt::trace!(
                            "No waker for received frame #{}, retrying receive later",
                            frame_index
                        );

                        retries.push_back(Retry {
                            retry_count: 1,
                            index: frame_index,
                            frame,
                        });

                        retries_high_water_mark = retries_high_water_mark.max(retries.len());
                    }
                    Err(e) => return Err(io::Error::other(e)),
                }
            } else {
                fmt::warn!("Tried to receive frame key {} more than once", key);
            }
        }

        if bufs.is_empty() && retries.is_empty() {
            fmt::trace!("No frames in flight, waiting to be woken with new frames to send");

            let start = Instant::now();

            // This must be after the send packet code as there can be a (safe!) race condition on
            // startup where the TX waker hasn't been registered yet, so when a future from another
            // thread tries to send its frame, it has no waker, so we just end up waiting forever.
            //
            // If this wait() is down here, we get at least one loop where any queued packets can be
            // sent.
            signal.wait();

            fmt::trace!("--> Waited for {} ns", start.elapsed().as_nanos());
        } else {
            fmt::trace!(
                "Buf keys {:?} and {} retries in flight",
                bufs.iter().map(|(k, _v)| k).collect::<Vec<_>>(),
                retries.len()
            );
        }
    }
}
