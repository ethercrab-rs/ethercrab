use crate::{PduRx, PduTx, error::Error, fmt, std::ParkSignal, std::unix::RawSocketDesc};
use core::{mem::MaybeUninit, task::Waker};
use io_uring::{IoUring, opcode};
use smallvec::{SmallVec, smallvec};
use std::{io, os::fd::AsRawFd, sync::Arc, time::Instant};

/// Use the upper bit of a u64 to mark whether a frame is a write (`1`) or a read (`0`).
const WRITE_MASK: u64 = 1 << 63;
const ENTRIES: usize = 256;

/// Create a blocking TX/RX loop using `io_uring`.
///
/// This function is only available on `linux` targets as it requires `io_uring` support. Older
/// kernels may not support `io_uring`.
/// this will choose whether or not to use multishot recveives based on the current kernel version,
/// for explicit choice, see [tx_rx_task_io_uring_read] and [tx_rx_task_io_uring_readmulti]
pub fn tx_rx_task_io_uring<'sto>(
    interface: &str,
    pdu_tx: PduTx<'sto>,
    pdu_rx: PduRx<'sto>,
) -> Result<(PduTx<'sto>, PduRx<'sto>), io::Error> {
    let mut ring = IoUring::new(ENTRIES as u32)?;
    let mut probe = io_uring::register::Probe::new();
    ring.submitter().register_probe(&mut probe)?;

    if probe.is_supported(opcode::ReadMulti::CODE) {
        tx_rx_task_io_uring_readmulti(interface, pdu_tx, pdu_rx, &mut ring)
    } else {
        tx_rx_task_io_uring_read(interface, pdu_tx, pdu_rx, &mut ring)
    }
}

pub fn tx_rx_task_io_uring_read<'sto>(
    interface: &str,
    mut pdu_tx: PduTx<'sto>,
    mut pdu_rx: PduRx<'sto>,
    ring: &mut IoUring,
) -> Result<(PduTx<'sto>, PduRx<'sto>), io::Error> {
    let mut socket = RawSocketDesc::new(interface)?;

    let mtu = socket.interface_mtu()?;

    fmt::debug!(
        "Opening {} with MTU {}, blocking, using io_uring",
        interface,
        mtu
    );

    // MTU is payload size. We need to add the layer 2 header which is 18 bytes.
    let mtu = mtu + 18;

    // SAFETY: Max entries is 256 because `PduStorage::N` is checked to be in 0..u8::MAX, and will
    // eventually be a `u8` once const generics get there. Twice as much space is reserved as each
    // frame requires a send _and_ receive buffer.
    //
    // This data MUST NOT MOVE or be reordered once created as io_uring holds pointers into it.
    let mut bufs: slab::Slab<(io_uring::squeue::Entry, SmallVec<[u8; 1518]>)> =
        slab::Slab::with_capacity(ENTRIES * 2);

    let mut high_water_mark = 0;

    let signal = Arc::new(ParkSignal::new());
    let waker = Waker::from(Arc::clone(&signal));

    loop {
        pdu_tx.replace_waker(&waker);

        let mut sent = 0;

        while let Some(frame) = pdu_tx.next_sendable_frame() {
            let idx = frame.storage_slot_index();

            let tx_b = bufs.vacant_entry();
            let tx_key = tx_b.key();
            let (tx_entry, tx_buf) = tx_b.insert((
                unsafe { MaybeUninit::zeroed().assume_init() },
                smallvec![0; mtu],
            ));

            frame
                .send_blocking(|data: &[u8]| {
                    *tx_entry = opcode::Write::new(
                        io_uring::types::Fd(socket.as_raw_fd()),
                        data.as_ptr(),
                        data.len() as _,
                    )
                    .build()
                    // Distinguish sent frames from received frames by using the upper bit of
                    // the user data as a flag.
                    .user_data(tx_key as u64 | WRITE_MASK);

                    // TODO: Zero copy
                    tx_buf
                        .get_mut(0..data.len())
                        .ok_or(Error::Internal)?
                        .copy_from_slice(data);

                    while unsafe { ring.submission().push(tx_entry).is_err() } {
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
                "Insert frame TX {:#04x}, key {}, RX key {}",
                idx,
                tx_key,
                rx_key
            );

            while unsafe { ring.submission().push(rx_entry).is_err() } {
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

            let received = Instant::now();

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
                while unsafe { ring.submission_shared().push(rx_entry).is_err() } {
                    // If the submission queue is full, flush it to the kernel
                    ring.submit().expect("Internal error, failed to submit ops");
                }
            } else {
                let (_entry, frame) = bufs.remove(key as usize);

                let frame_index = frame
                    .get(0x11)
                    .ok_or_else(|| io::Error::other(Error::Internal))?;

                fmt::trace!(
                    "Raw frame {:#04x} result {} buffer key {}",
                    frame_index,
                    recv.result(),
                    key,
                );

                pdu_rx.receive_frame(&frame).map_err(io::Error::other)?;

                fmt::trace!("Received frame in {} ns", received.elapsed().as_nanos());
            }
        }

        if bufs.is_empty() {
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

            if pdu_tx.should_exit() {
                fmt::debug!("io_uring TX/RX was asked to exit");

                return Ok((pdu_tx.release(), pdu_rx.release()));
            }
        } else {
            fmt::trace!(
                "Buf keys {:?} in flight",
                bufs.iter().map(|(k, _v)| k).collect::<Vec<_>>(),
            );
        }
    }
}

pub fn tx_rx_task_io_uring_readmulti<'sto>(
    interface: &str,
    mut pdu_tx: PduTx<'sto>,
    mut pdu_rx: PduRx<'sto>,
    ring: &mut IoUring,
) -> Result<(PduTx<'sto>, PduRx<'sto>), io::Error> {
    let mut socket = RawSocketDesc::new(interface)?;
    let mtu = socket.interface_mtu()?;

    fmt::debug!(
        "Opening {} with MTU {}, blocking, using io_uring readmulti",
        interface,
        mtu
    );

    // MTU is payload size. We need to add the layer 2 header which is 18 bytes.
    let mtu = mtu + 18;

    // SAFETY: Max entries is 256 because `PduStorage::N` is checked to be in 0..u8::MAX, and will
    // eventually be a `u8` once const generics get there. Twice as much space is reserved as each
    // frame requires a send _and_ receive buffer.
    //
    // This data MUST NOT MOVE or be reordered once created as io_uring holds pointers into it.
    const ENTRIES: usize = 256;
    let mut tx_bufs: slab::Slab<(io_uring::squeue::Entry, SmallVec<[u8; 1518]>)> =
        slab::Slab::with_capacity(ENTRIES * 2);

    let mut high_water_mark = 0;

    let signal = Arc::new(ParkSignal::new());
    let waker = Waker::from(Arc::clone(&signal));

    use io_uring::types::BufRingEntry;

    // initalize the mmaped buffer
    let mmap_size = ENTRIES * (mtu + core::mem::size_of::<BufRingEntry>());

    let mmap_flags = libc::MAP_ANONYMOUS | libc::MAP_PRIVATE | libc::MAP_POPULATE;

    let base = unsafe {
        match libc::mmap(
            core::ptr::null_mut(),
            mmap_size,
            libc::PROT_READ | libc::PROT_WRITE,
            mmap_flags,
            -1,
            0,
        ) {
            libc::MAP_FAILED => return Err(io::Error::last_os_error()),
            addr => addr,
        }
    };

    let buffer_base: *const u8 = unsafe {
        base.offset((ENTRIES * core::mem::size_of::<BufRingEntry>()) as isize) as *const _
    };

    let base = base as *mut BufRingEntry;

    use std::sync::atomic::{AtomicU16, Ordering};

    unsafe {
        let buffer_tail = BufRingEntry::tail(base);
        let _ = AtomicU16::from_ptr(buffer_tail as _).store(0, Ordering::Relaxed);
    }

    // register the buffer to io_uring
    let bgid = 1;

    unsafe {
        ring.submitter()
            .register_buf_ring(base as u64, ENTRIES as _, bgid)?;
    }

    // initalize the buffer

    let mask = ENTRIES - 1;
    for offset in 0..ENTRIES {
        let buf_id = offset;
        let (entry, buffer_addr) = unsafe {
            let ring_tail = BufRingEntry::tail(base);
            let offset = ((core::ptr::addr_of!(ring_tail) as u32) + offset as u32) & (mask as u32);
            (
                &mut *base.offset(offset as isize),
                buffer_base.offset(((offset as usize) * mtu) as isize),
            )
        };
        entry.set_addr(buffer_addr as u64);
        entry.set_len(mtu as _);
        entry.set_bid(buf_id as _);
    }

    unsafe {
        let tail = BufRingEntry::tail(base);
        let _ = AtomicU16::from_ptr(tail as _).fetch_add(ENTRIES as _, Ordering::Relaxed);
    }

    // finally, register a readmulti to io_uring
    let rx_multi_entry =
        opcode::ReadMulti::new(io_uring::types::Fd(socket.as_raw_fd()), mtu as _, bgid).build();

    while unsafe { ring.submission().push(&rx_multi_entry).is_err() } {
        // If the submission queue is full, flush it to the kernel
        ring.submit().expect("Internal error, failed to submit ops");
    }

    loop {
        pdu_tx.replace_waker(&waker);

        let mut sent = 0;

        while let Some(frame) = pdu_tx.next_sendable_frame() {
            let idx = frame.storage_slot_index();

            let tx_b = tx_bufs.vacant_entry();
            let tx_key = tx_b.key();
            let (tx_entry, tx_buf) = tx_b.insert((
                unsafe { MaybeUninit::zeroed().assume_init() },
                smallvec![0; mtu],
            ));

            frame
                .send_blocking(|data: &[u8]| {
                    *tx_entry = opcode::Write::new(
                        io_uring::types::Fd(socket.as_raw_fd()),
                        data.as_ptr(),
                        data.len() as _,
                    )
                    .build()
                    // Distinguish sent frames from received frames by using the upper bit of
                    // the user data as a flag.
                    .user_data(tx_key as u64 | WRITE_MASK);

                    // TODO: Zero copy
                    tx_buf
                        .get_mut(0..data.len())
                        .ok_or(Error::Internal)?
                        .copy_from_slice(data);

                    while unsafe { ring.submission().push(tx_entry).is_err() } {
                        // If the submission queue is full, flush it to the kernel
                        ring.submit().expect("Internal error, failed to submit ops");
                    }

                    sent += 1;

                    Ok(data.len())
                })
                .expect("Send blocking");

            fmt::trace!("Insert frame TX {:#04x}, key {}", idx, tx_key,);

            high_water_mark = high_water_mark.max(tx_bufs.len());
        }

        // TODO: Collect these metrics for later gathering instead of just asserting
        // assert_eq!(ring.completion().overflow(), 0);
        // assert_eq!(ring.completion().is_full(), false);
        // assert_eq!(ring.submission().cq_overflow(), false);
        // assert_eq!(ring.submission().dropped(), 0);

        ring.submission().sync();
        let now = Instant::now();

        if sent > 0 {
            ring.submit()?;
        }

        fmt::trace!(
            "Submitted, waited for {} completions for {} us",
            ring.completion().len(),
            now.elapsed().as_micros(),
        );

        for recv in ring.completion() {
            if recv.result() < 0 {
                return Err(io::Error::last_os_error());
            }

            let key = recv.user_data();

            let received = Instant::now();

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
                tx_bufs.remove(key as usize);

                continue;
            }

            const IORING_CQE_F_BUFFER: libc::c_uint = 1;
            const IORING_CQE_BUFFER_SHIFT: libc::c_uint = 16;

            let flags = recv.flags();
            let buf_id = if flags & IORING_CQE_F_BUFFER == 0 {
                panic!("this should not happen");
            } else {
                (flags >> IORING_CQE_BUFFER_SHIFT) as u16
            };

            let len = recv.result() as usize;

            let frame = unsafe {
                let raw_buf = buffer_base.offset((buf_id as usize * mtu) as isize);
                core::slice::from_raw_parts(raw_buf, len)
            };

            // successful read: increment the buffer ring
            unsafe {
                let tail = BufRingEntry::tail(base);
                let _ = AtomicU16::from_ptr(tail as _).fetch_add(1, Ordering::Relaxed);
            }

            let frame_index = frame
                .get(0x11)
                .ok_or_else(|| io::Error::other(Error::Internal))?;

            fmt::trace!(
                "Raw frame {:#04x} result {} buffer key {}",
                frame_index,
                recv.result(),
                key,
            );

            pdu_rx.receive_frame(frame).map_err(io::Error::other)?;

            fmt::trace!("Received frame in {} ns", received.elapsed().as_nanos());
        }
    }
}
