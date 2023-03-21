//! Items to use when not in `no_std` environments.

mod raw_socket;

use self::raw_socket::RawSocketDesc;
use crate::{
    error::Error,
    pdu_loop::{PduRx, PduTx},
};
use core::{future::Future, pin::Pin, task::Poll};
use smol::io::{AsyncRead, AsyncWrite};
use smol::Async;

struct TxRxFut<'a> {
    socket: Async<RawSocketDesc>,
    tx: PduTx<'a>,
    rx: PduRx<'a>,
}

impl Future for TxRxFut<'_> {
    type Output = Result<(), Error>;

    fn poll(mut self: Pin<&mut Self>, ctx: &mut core::task::Context<'_>) -> Poll<Self::Output> {
        let mut buf = [0u8; 1536];

        // Lock the waker so we don't poll concurrently. spin::RwLock does this atomically
        let mut waker = self.tx.lock_waker();

        while let Some(frame) = self.tx.next_sendable_frame() {
            // FIXME: Release frame on failure
            let data = frame.write_ethernet_packet(&mut buf)?;

            match Pin::new(&mut self.socket).poll_write(ctx, data) {
                Poll::Ready(Ok(bytes_written)) => {
                    if bytes_written != data.len() {
                        log::error!("Only wrote {} of {} bytes", bytes_written, data.len());

                        // FIXME: Release frame

                        // TODO: Better error
                        return Poll::Ready(Err(Error::SendFrame));
                    }

                    frame.mark_sent();
                }
                // TODO: Return a better error type
                // FIXME: Release frame on failure
                Poll::Ready(Err(e)) => {
                    log::error!("Send PDU failed: {e}");

                    return Poll::Ready(Err(Error::SendFrame));
                }
                Poll::Pending => (),
            }
        }

        match Pin::new(&mut self.socket).poll_read(ctx, &mut buf) {
            Poll::Ready(Ok(n)) => {
                let packet = &buf[0..n];

                // FIXME: Release frame on failure
                self.rx.receive_frame(packet).expect("bad news");
            }
            // TODO: Return a better error type
            // FIXME: Release frame on failure
            Poll::Ready(Err(e)) => {
                log::error!("Receive PDU failed: {e}");

                return Poll::Ready(Err(Error::SendFrame));
            }
            Poll::Pending => (),
        }

        // self.tx.set_waker(ctx.waker());
        waker.replace(ctx.waker().clone());

        Poll::Pending
    }
}

/// Spawn a TX and RX task.
pub fn tx_rx_task(
    interface: &str,
    pdu_tx: PduTx<'static>,
    pdu_rx: PduRx<'static>,
) -> Result<impl Future<Output = Result<(), Error>>, std::io::Error> {
    let socket = RawSocketDesc::new(interface)?;

    let async_socket = smol::Async::new(socket)?;

    let task = TxRxFut {
        socket: async_socket,
        tx: pdu_tx,
        rx: pdu_rx,
    };

    Ok(task)
}
