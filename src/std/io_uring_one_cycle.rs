use crate::{PduRx, PduTx, error::Error, fmt, std::ParkSignal, std::unix::RawSocketDesc};
use core::{mem::MaybeUninit, task::Waker};
use io_uring::{IoUring, opcode};
use smallvec::{SmallVec, smallvec};
use std::{io, os::fd::AsRawFd, sync::Arc, time::Instant};

/// Use the upper bit of a u64 to mark whether a frame is a write (`1`) or a read (`0`).
const WRITE_MASK: u64 = 1 << 63;
const ENTRIES: usize = 256;

pub struct TX_RX_TASK_IO_URING {
    socket_desc: RawSocketDesc,
    mtu: usize,
    signal: Arc<ParkSignal>,
    bufs: slab::Slab<(io_uring::squeue::Entry, SmallVec<[u8; 1518]>)>,
    ring: IoUring,
    waker: Waker,
}

pub fn setup_tx_rx_task(interface: &str) -> Result<TX_RX_TASK_IO_URING, io::Error> {
    let mut socket = RawSocketDesc::new(interface)?;
    // MTU is payload size. We need to add the layer 2 header which is 18 bytes.
    let mtu = socket.interface_mtu()?;
    let mtu = mtu + 18;

    let mut bufs: slab::Slab<(io_uring::squeue::Entry, SmallVec<[u8; 1518]>)> =
        slab::Slab::with_capacity(ENTRIES * 2);

    let mut ring = IoUring::new(ENTRIES as u32)?;

    // checks io_uring support for used opcodes
    let mut probe = io_uring::register::Probe::new();
    ring.submitter().register_probe(&mut probe)?;
    if !(probe.is_supported(opcode::Read::CODE) && probe.is_supported(opcode::Write::CODE)) {
        log::error!("io_uring does not support read and/or write opcodes");
        return Err(io::Error::other(Error::Internal));
    }

    let mut high_water_mark = 0;

    let signal = Arc::new(ParkSignal::new());
    let waker = Waker::from(Arc::clone(&signal));

    return Ok(TX_RX_TASK_IO_URING {
        socket_desc: socket,
        mtu,
        signal: signal,
        bufs,
        ring,
        waker,
    });
}

/// Create a blocking TX/RX loop using `io_uring`.
///
/// This function is only available on `linux` targets as it requires `io_uring` support. Older
/// kernels may not support `io_uring`.
pub fn tx_rx_task_io_uring_cycle<'sto>(
    config: &mut TX_RX_TASK_IO_URING,
    mut pdu_tx: PduTx<'sto>,
    mut pdu_rx: PduRx<'sto>,
) -> Result<(PduTx<'sto>, PduRx<'sto>), io::Error> {
    pdu_tx.replace_waker(&config.waker);

    let mut sent = 0;

    // === submit all TX frames and corresponding RX frames ===
    while let Some(frame) = pdu_tx.next_sendable_frame() {
        let tx_b = config.bufs.vacant_entry();
        let tx_key = tx_b.key();
        let (tx_entry, tx_buf) = tx_b.insert((
            unsafe { MaybeUninit::zeroed().assume_init() },
            smallvec![0; config.mtu],
        ));

        let res = frame.send_blocking(|data: &[u8]| {
            *tx_entry = opcode::Write::new(
                io_uring::types::Fd(config.socket_desc.as_raw_fd()),
                data.as_ptr(),
                data.len() as _,
            )
            .build()
            .user_data(tx_key as u64 | WRITE_MASK);

            tx_buf[..data.len()].copy_from_slice(data);

            while unsafe { config.ring.submission().push(tx_entry).is_err() } {
                config.ring.submit().expect("submit failed");
            }

            sent += 1;
            Ok(data.len())
        });

        let rx_b = config.bufs.vacant_entry();
        let rx_key = rx_b.key();
        let (rx_entry, rx_buf) = rx_b.insert((
            unsafe { MaybeUninit::zeroed().assume_init() },
            smallvec![0; config.mtu],
        ));

        *rx_entry = opcode::Read::new(
            io_uring::types::Fd(config.socket_desc.as_raw_fd()),
            rx_buf.as_mut_ptr() as _,
            rx_buf.len() as _,
        )
        .build()
        .user_data(rx_key as u64);

        while unsafe { config.ring.submission().push(rx_entry).is_err() } {
            config.ring.submit().expect("submit failed");
        }
    }

    config.ring.submission().sync();

    if sent > 0 {
        config.ring.submit_and_wait(sent * 2)?;
    }

    // === process completions once ===
    for recv in unsafe { config.ring.completion_shared() } {
        let key = recv.user_data();
        if key & WRITE_MASK == WRITE_MASK {
            config.bufs.remove((key & !WRITE_MASK) as usize);
            continue;
        }
        if recv.result() < 0 && recv.result() != -libc::EWOULDBLOCK {
            return Err(io::Error::last_os_error());
        }
        let (_entry, frame) = config.bufs.remove(key as usize);
        pdu_rx.receive_frame(&frame).map_err(io::Error::other)?;
    }

    Ok((pdu_tx, pdu_rx))
}
