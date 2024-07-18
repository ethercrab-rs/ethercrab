use crate::{
    error::{Error, PduError},
    fmt,
    std::unix::RawSocketDesc,
    PduRx, PduTx,
};
use core::{mem::MaybeUninit, num::NonZeroU32, str::FromStr, task::Waker};
use io_uring::{opcode, IoUring};
use smallvec::{smallvec, SmallVec};
use smoltcp::wire::EthernetProtocol;
use std::{
    io::{self, Write},
    os::fd::AsRawFd,
    sync::Arc,
    task::Wake,
    thread::{self, Thread},
    time::Instant,
};
use xsk_rs::{
    config::{Interface, SocketConfig, UmemConfig, XdpFlags},
    CompQueue, FillQueue, FrameDesc, RxQueue, TxQueue, Umem,
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
        self.current_thread.unpark();
    }
}

/// TODO: Docs
pub fn tx_rx_task_xdp<'sto>(
    interface: &str,
    mut pdu_tx: PduTx<'sto>,
    mut pdu_rx: PduRx<'sto>,
) -> Result<(), io::Error> {
    let mut socket = RawSocketDesc::new(interface)?;

    let mtu = socket.interface_mtu()?;

    fmt::debug!(
        "Opening {} with MTU {}, blocking, using XDP",
        interface,
        mtu
    );

    // MTU is payload size. We need to add the layer 2 header which is 18 bytes.
    let mtu = mtu + 18;
    let frame_count = 32.try_into().expect("Non-zero frame count required");

    let signal = Arc::new(ParkSignal::new());
    let waker = Waker::from(Arc::clone(&signal));

    let mut xsk_tx = build_socket_and_umem(
        UmemConfig::default(),
        // TODO: Config option to use `XDP_FLAGS_DRV_MODE` or `XDP_FLAGS_HW_MODE` (driver and NIC
        // mode respectively)
        SocketConfig::builder()
            .xdp_flags(XdpFlags::XDP_FLAGS_SKB_MODE)
            .build(),
        frame_count,
        &Interface::from_str(interface)?,
        0,
    );

    let tx_umem = &xsk_tx.umem;
    let mut tx_descs = &mut xsk_tx.descs;

    let mut in_flight = 0u32;

    loop {
        pdu_tx.replace_waker(&waker);

        let mut tx_frame_count = 0;

        while let Some(frame) = pdu_tx.next_sendable_frame() {
            let idx = frame.index();

            let mut it = tx_descs.iter_mut();

            frame
                .send_blocking(|data: &[u8]| {
                    fmt::trace!("Queuing frame to send");

                    let descriptor = it.next().ok_or_else(|| {
                        fmt::error!("Not enough send slots available");

                        Error::SendFrame
                    })?;

                    unsafe { tx_umem.data_mut(descriptor) }
                        .cursor()
                        .write_all(data)
                        .map_err(|e| {
                            fmt::error!("Failed to write frame data: {}", e);

                            Error::SendFrame
                        })?;

                    Ok(data.len())
                })
                .expect("Send blocking");

            in_flight += 1;

            tx_frame_count += 1;
        }

        // let tx_frames_sent =
        //     unsafe { xsk_tx.tx_q.produce_and_wakeup(&tx_descs[0..tx_frame_count]) }?;

        // Add consumed frames back to the tx queue
        while unsafe {
            xsk_tx
                .tx_q
                .produce_and_wakeup(&tx_descs[0..tx_frame_count])
                .unwrap()
        } != tx_frame_count
        {
            // Loop until frames added to the tx ring.
            log::debug!("Sender TX queue failed to allocate");
        }

        if tx_frame_count > 0 {
            fmt::debug!("Sent {} frame(s)", tx_frame_count,);

            let frames_filled = unsafe { xsk_tx.fq.produce(&tx_descs[0..tx_frame_count]) };

            fmt::debug!("--> Filled queue with {} frames", frames_filled);
        }

        // ---
        // Receive
        // ---

        let pkts_recvd = unsafe { xsk_tx.rx_q.poll_and_consume(&mut tx_descs, 0).unwrap() };

        for recv_desc in tx_descs.iter_mut().take(pkts_recvd) {
            let received = Instant::now();

            let data = unsafe { tx_umem.data(recv_desc) };

            let frame_index = data
                .get(0x11)
                .ok_or_else(|| io::Error::other(Error::Internal))?;

            loop {
                match pdu_rx.receive_frame(&data) {
                    Ok(()) => break,
                    Err(Error::Pdu(PduError::NoWaker)) => {
                        fmt::trace!(
                            "No waker for received frame {:#04x}, retrying receive",
                            frame_index
                        );

                        thread::yield_now();
                    }
                    Err(e) => return Err(io::Error::other(e)),
                }
            }

            unsafe { xsk_tx.cq.consume_one(recv_desc) };

            fmt::trace!("Received frame in {} ns", received.elapsed().as_nanos());

            in_flight = in_flight
                .checked_sub(1)
                .expect("Can't have fewer than 0 frames in flight");
        }

        if in_flight == 0 {
            fmt::trace!("Nothing to send, waiting for wakeup");

            let start = Instant::now();

            signal.wait();

            fmt::trace!("--> Waited for {} ns", start.elapsed().as_nanos());
        }
    }
}

pub fn build_socket_and_umem(
    umem_config: UmemConfig,
    socket_config: SocketConfig,
    frame_count: NonZeroU32,
    if_name: &Interface,
    queue_id: u32,
) -> Xsk {
    let (umem, frames) = Umem::new(umem_config, frame_count, false).expect("failed to build umem");

    let (tx_q, rx_q, fq_and_cq) = unsafe {
        xsk_rs::Socket::new(socket_config, &umem, if_name, queue_id)
            .expect("failed to build socket")
    };

    let (fq, cq) = fq_and_cq.expect(&format!(
        "missing fill and comp queue - interface {:?} may already be bound to",
        if_name
    ));

    Xsk {
        umem,
        fq,
        cq,
        tx_q,
        rx_q,
        descs: frames,
    }
}

pub struct Xsk {
    pub umem: Umem,
    pub fq: FillQueue,
    pub cq: CompQueue,
    pub tx_q: TxQueue,
    pub rx_q: RxQueue,
    pub descs: Vec<FrameDesc>,
}
