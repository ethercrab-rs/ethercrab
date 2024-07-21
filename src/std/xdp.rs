use crate::{
    error::{Error, PduError},
    fmt,
    pdu_loop::ReceiveAction,
    std::unix::RawSocketDesc,
    PduRx, PduTx,
};
use core::{num::NonZeroU32, str::FromStr, task::Waker};
use std::{
    io::{self, Write},
    sync::Arc,
    task::Wake,
    thread::{self, Thread},
    time::Instant,
};
use xsk_rs::{
    config::{BindFlags, Interface, SocketConfig, UmemConfig},
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

/// Start a TX/RX task using XDP (Linux only).
///
/// Note that running this function on the same core as other EtherCrab code will cause a lockup.
/// Use [`core_affinity`] or other means to move the thread that executes this function to a
/// different core.
///
/// Using XDP requires some build-time dependencies. These can be installed on `deb`-based distros
/// as follows:
///
/// ```bash
/// sudo apt install build-essential m4 clang bpftool libelf-dev libpcap-dev
/// ```
///
/// Ubuntu 22.04 does not provide a `bpftool` package. Instead, install `linux-tools-common
/// linux-tools-$(uname -r)`.
///
/// It may also be necessary to symlink some folders to mitigate an error around `asm/types.h` not
/// being found:
///
/// ```bash
/// sudo ln -s /usr/include/asm-generic/ /usr/include/asm
/// ```
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

    let frame_count = (pdu_tx.capacity() as u32)
        .try_into()
        .expect("Non-zero frame count required");

    fmt::debug!("Frame count {}", frame_count);

    let signal = Arc::new(ParkSignal::new());
    let waker = Waker::from(Arc::clone(&signal));

    let config = SocketConfig::builder()
        .bind_flags(BindFlags::XDP_USE_NEED_WAKEUP)
        .build();

    let mut xsk = build_socket_and_umem(
        UmemConfig::default(),
        config,
        frame_count,
        &Interface::from_str(interface)?,
        0,
    );

    let umem = &xsk.umem;
    let mid = xsk.descs.len() / 2;
    let (tx_descs, mut rx_descs) = xsk.descs.split_at_mut(mid);

    // Make receive slots available
    unsafe { xsk.fq.produce(&mut rx_descs) };

    // Clear RX buffers before starting up
    while unsafe { xsk.rx_q.poll_and_consume(&mut rx_descs, 0).unwrap() } > 0 {}

    let mut in_flight = 0u32;

    loop {
        pdu_tx.replace_waker(&waker);

        let mut tx_frame_count = 0;

        let mut it = tx_descs.iter_mut();

        while let Some(frame) = pdu_tx.next_sendable_frame() {
            let idx = frame.index();

            frame
                .send_blocking(|data: &[u8]| {
                    let descriptor = it.next().ok_or_else(|| {
                        fmt::error!("Not enough send slots available");

                        Error::SendFrame
                    })?;

                    fmt::debug!(
                        "Queuing frame at index {} (EtherCAT PDU {:#04x}) to send in descriptor {}",
                        idx,
                        data[0x11],
                        descriptor.addr()
                    );

                    unsafe { umem.data_mut(descriptor) }
                        .cursor()
                        .write_all(data)
                        .map_err(|e| {
                            fmt::error!("Failed to write frame data: {}", e);

                            Error::SendFrame
                        })?;

                    unsafe { xsk.tx_q.produce_one(descriptor) };

                    Ok(data.len())
                })
                .expect("Send blocking");

            in_flight += 1;

            tx_frame_count += 1;
        }

        let sent_descs = &mut tx_descs[0..tx_frame_count];

        if tx_frame_count > 0 {
            if xsk.tx_q.needs_wakeup() {
                xsk.tx_q.wakeup()?;
            }

            fmt::trace!("Sent {} frame(s)", tx_frame_count);

            // Wait until all packets have been sent
            loop {
                let frames_filled = unsafe { xsk.cq.consume(sent_descs) };

                fmt::trace!("--> Completion queue filled with {} frames", frames_filled);

                if frames_filled == tx_frame_count {
                    break;
                }
            }
        }

        // ---
        // Receive
        // ---

        // Take ownership of any received descriptors back from the kernel and mark them as ready
        // for reuse.
        // SAFETY: The descriptors could potentially be reused from underneath us if we don't do
        // this on a single thread; the code below parses the frames and copies their contents into
        // other memory, so as long as it's done by the time more packets are received, we're good.
        let pkts_recvd = unsafe { xsk.rx_q.poll_and_consume(&mut rx_descs, 0).unwrap() };

        for recv_desc in rx_descs.iter_mut().take(pkts_recvd) {
            let received = Instant::now();

            let data = unsafe { umem.data(recv_desc) };

            let frame_first_pdu_index = data
                .get(0x11)
                .ok_or_else(|| io::Error::other(Error::Internal))?;

            fmt::debug!(
                "Received frame {:#04x} in descriptor {}",
                frame_first_pdu_index,
                recv_desc.addr()
            );

            loop {
                match pdu_rx.receive_frame(&data) {
                    Ok(action) => {
                        // Return descriptor back to fill queue to receive another packet with
                        unsafe { xsk.fq.produce_one(&recv_desc) };

                        if action == ReceiveAction::Processed {
                            fmt::trace!(
                                "--> Processed received frame with PDU {:#04x} in {} ns",
                                frame_first_pdu_index,
                                received.elapsed().as_nanos()
                            );

                            in_flight = in_flight
                                .checked_sub(1)
                                .expect("Can't have fewer than 0 frames in flight");
                        } else {
                            fmt::trace!("--> Frame ignored");
                        }

                        break;
                    }
                    Err(Error::Pdu(PduError::NoWaker)) => {
                        fmt::trace!(
                            "--> No waker for received frame PDU {:#04x}, retrying receive",
                            frame_first_pdu_index
                        );

                        thread::yield_now();
                    }
                    Err(e) => return Err(io::Error::other(e)),
                }
            }
        }

        if in_flight == 0 {
            fmt::debug!("Nothing to send, waiting for wakeup");

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
