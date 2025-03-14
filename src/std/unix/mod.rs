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
    error::Error,
    fmt,
    pdu_loop::{PduRx, PduTx},
};
use async_io::Async;
use core::{future::Future, pin::Pin, task::Poll};
use futures_lite::{AsyncRead, AsyncWrite};

struct TxRxFut<'a> {
    socket: Async<RawSocketDesc>,
    mtu: usize,
    tx: Option<PduTx<'a>>,
    rx: Option<PduRx<'a>>,
}

impl<'a> Future for TxRxFut<'a> {
    type Output = Result<(PduTx<'a>, PduRx<'a>), Error>;

    fn poll(mut self: Pin<&mut Self>, ctx: &mut core::task::Context<'_>) -> Poll<Self::Output> {
        unsafe {
            // Re-register waker to make sure this future is polled again
            self.tx
                .as_mut()
                .unwrap_unchecked()
                .replace_waker(ctx.waker());

            if self.tx.as_mut().unwrap_unchecked().should_exit() {
                fmt::debug!("TX/RX future was asked to exit");

                return Poll::Ready(Ok((
                    self.tx.take().unwrap().release(),
                    self.rx.take().unwrap().release(),
                )));
            }
        }

        while let Some(frame) = unsafe { self.tx.as_mut().unwrap_unchecked() }.next_sendable_frame()
        {
            let res = frame.send_blocking(|data| {
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

                let packet = buf.get(0..n).ok_or(Error::Internal)?;

                if n == 0 {
                    fmt::warn!("Received zero bytes");
                }

                if let Err(e) = unsafe { self.rx.as_mut().unwrap_unchecked() }.receive_frame(packet)
                {
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
) -> Result<impl Future<Output = Result<(PduTx<'sto>, PduRx<'sto>), Error>> + 'sto, std::io::Error>
{
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

    let async_socket = Async::new(socket)?;

    let task = TxRxFut {
        socket: async_socket,
        mtu,
        tx: Some(pdu_tx),
        rx: Some(pdu_rx),
    };

    Ok(task)
}

/// Get the current time in nanoseconds from the EtherCAT epoch, 2000-01-01.
///
/// On POSIX systems, this function uses the monotonic clock provided by the system.
pub fn ethercat_now() -> u64 {
    let mut time = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };

    unsafe {
        libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut time);
    };

    let t = (time.tv_sec as u64) * 1_000_000_000 + (time.tv_nsec as u64);

    // EtherCAT epoch is 2000-01-01
    t.saturating_sub(946684800)
}

// Unix only
#[allow(trivial_numeric_casts)]
fn ifreq_for(name: &str) -> ifreq {
    let mut ifreq = ifreq {
        ifr_name: [0; libc::IF_NAMESIZE],
        ifr_data: 0,
    };
    for (i, byte) in name.as_bytes().iter().enumerate() {
        ifreq.ifr_name[i] = *byte as libc::c_char;
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
