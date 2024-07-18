use crate::{
    error::{Error, PduError},
    fmt,
    std::unix::RawSocketDesc,
    PduRx, PduTx,
};
use core::{mem::MaybeUninit, num::NonZeroU32, str::FromStr, task::Waker};
use io_uring::{opcode, IoUring};
use smallvec::{smallvec, SmallVec};
use std::{
    io::{self, Write},
    os::fd::AsRawFd,
    sync::Arc,
    task::Wake,
    thread::{self, Thread},
    time::Instant,
};
use xsk_rs::{
    config::{Interface, SocketConfig, UmemConfig},
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

    let signal = Arc::new(ParkSignal::new());
    let waker = Waker::from(Arc::clone(&signal));

    let mut xsk_tx = build_socket_and_umem(
        UmemConfig::default(),
        SocketConfig::default(),
        32.try_into().expect("Non-zero frame count required"),
        &Interface::from_str(interface)?,
        0,
    );

    let tx_umem = &xsk_tx.umem;
    let tx_descs = &mut xsk_tx.descs;

    loop {
        pdu_tx.replace_waker(&waker);

        let mut tx_frame_count = 0;

        while let Some(frame) = pdu_tx.next_sendable_frame() {
            let idx = frame.index();

            let mut it = tx_descs.iter_mut();

            frame
                .send_blocking(|data: &[u8]| {
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

            tx_frame_count += 1;
        }

        let tx_frames_sent =
            unsafe { xsk_tx.tx_q.produce_and_wakeup(&tx_descs[0..tx_frame_count]) }?;

        fmt::debug!(
            "Wanted to send {} frames, actually sent {}",
            tx_frame_count,
            tx_frames_sent
        );

        // // Handle tx
        // match unsafe { xsk_tx.cq.consume(&mut tx_descs[..]) } {
        //     0 => {
        //         fmt::debug!("Sender completion queue consumed 0 frames");

        //         if xsk_tx.tx_q.needs_wakeup() {
        //             fmt::debug!("Waking sender TX queue");

        //             xsk_tx.tx_q.wakeup().unwrap();
        //         }
        //     }
        //     frames_rcvd => {
        //         fmt::debug!("Sender comp queue consumed {} frames", frames_rcvd);

        //         // Wait until we're ok to write
        //         while !xsk_tx.tx_q.poll(0).unwrap() {
        //             fmt::debug!("Sender socket not ready to write");

        //             continue;
        //         }

        //         // TODO: Configurable batch size
        //         let frames_to_send = frames_rcvd.min(64);

        //         // Add consumed frames back to the tx queue
        //         while unsafe {
        //             xsk_tx
        //                 .tx_q
        //                 .produce_and_wakeup(&tx_descs[..frames_to_send])
        //                 .unwrap()
        //         } != frames_to_send
        //         {
        //             // Loop until frames added to the tx ring.
        //             fmt::debug!("Sender tx queue failed to allocate");
        //         }
        //         fmt::debug!("Submitted {} frames to sender TX queue", frames_to_send);
        //     }
        // }

        // TODO: Receive

        let start = Instant::now();

        signal.wait();

        fmt::trace!("--> Waited for {} ns", start.elapsed().as_nanos());
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
