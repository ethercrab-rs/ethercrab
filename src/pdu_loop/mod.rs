mod frame_element;
mod frame_header;
mod pdu_flags;
mod pdu_header;
mod pdu_rx;
mod pdu_tx;
// NOTE: Pub so doc links work
pub mod storage;

use crate::{command::Command, error::Error, pdu_loop::storage::PduStorageRef};
pub use frame_element::received_frame::ReceivedPdu;
pub use frame_element::sendable_frame::SendableFrame;
pub use pdu_rx::PduRx;
pub use pdu_tx::PduTx;
pub use storage::PduStorage;

#[cfg(feature = "__internals")]
pub use frame_header::EthercatFrameHeader;
#[cfg(feature = "__internals")]
pub use pdu_header::PduHeader;

/// Unused PDU marker.
///
/// The value this is set to should be larger than `u8::MAX` to ensure no valid frame index can
/// equal it.
const PDU_UNUSED_SENTINEL: u16 = u16::MAX;

const PDU_SLOTS: usize = 256;

pub type PduResponse<T> = (T, u16);

pub trait CheckWorkingCounter<T> {
    fn wkc(self, expected: u16) -> Result<T, Error>;

    /// Ignores working counter if it is `None`.
    fn maybe_wkc(self, expected: Option<u16>) -> Result<T, Error>;
}

impl<T> CheckWorkingCounter<T> for PduResponse<T> {
    fn wkc(self, expected: u16) -> Result<T, Error> {
        if self.1 == expected {
            Ok(self.0)
        } else {
            Err(Error::WorkingCounter {
                expected,
                received: self.1,
            })
        }
    }

    fn maybe_wkc(self, expected: Option<u16>) -> Result<T, Error> {
        match expected {
            Some(expected) => self.wkc(expected),
            None => Ok(self.0),
        }
    }
}

/// The core EtherCrab network communications driver.
///
/// This item orchestrates queuing, sending and receiving responses to individual PDUs. It uses a
/// fixed length list of frame slots which are cycled through sequentially to ensure each PDU packet
/// has a unique ID (by using the slot index).
///
/// Use [`PduTx`] and [`PduRx`] to integrate EtherCrab into network drivers.
///
/// # High level overview
///
/// <img alt="High level overview of the PDU loop send/receive process" style="background: white" src="https://mermaid.ink/svg/pako:eNplkcFuwjAMhl8lyrkT9x64jOsmBEzqoRcrcUtEm2SJA0KId5_dFmlbc3GUfP79235oEyzqWmf8LugN7hz0CcbWKz4REjnjInhSBoZBQVbvHJ3vleStqf3uSyAJQwhxDZyaQyPEqdnwhc4Jwa6pT6RbSJf5Y6r8tt2Kaq2O6C0XH0fwdmOBYIakojCizxBBj6rjRrBSN7ggv08xzfTkQvCl0CIbwVyQFAVl8eoM5pleoF_6BzTorrgk_NOcbO4hZVQJcww-v0x0hUrCv4alOxGcQSXzuIuDklFXesQ0grO8oIektZrOOGKra75a4Anp1j-Zg0LhePdG15QKVrpEHs1rmbruYGATGq2jkD7mjU-Lf_4AMq-oMQ" />
///
/// Source (MermaidJS)
///
/// ```mermaid
/// sequenceDiagram
///     participant call as Calling code
///     participant PDU as PDU loop
///     participant TXRX as TX/RX thread
///     participant Network
///     call ->> PDU: Send command/data
///     PDU ->> TXRX: Stage frame, wake TX waker
///     TXRX ->> Network: Send packet to devices
///     Network ->> TXRX: Receive packet
///     TXRX ->> PDU: Parse response, wake future
///     PDU ->> call: Response ready to use
/// ```
#[derive(Debug)]
pub struct PduLoop<'sto> {
    // TODO: Un-pub
    pub(crate) storage: PduStorageRef<'sto>,
}

impl<'sto> PduLoop<'sto> {
    /// Create a new PDU loop with the given backing storage.
    pub(in crate::pdu_loop) const fn new(storage: PduStorageRef<'sto>) -> Self {
        assert!(storage.num_frames <= u8::MAX as usize);

        Self { storage }
    }

    pub(crate) fn max_frame_data(&self) -> usize {
        self.storage.frame_data_len
    }

    /// Tell the packet sender there are PDUs ready to send.
    pub(crate) fn wake_sender(&self) {
        self.storage.tx_waker.wake();
    }

    /// Broadcast (BWR) a packet full of zeroes, up to `payload_length`.
    pub(crate) async fn pdu_broadcast_zeros(
        &self,
        register: u16,
        payload_length: u16,
    ) -> Result<(), Error> {
        let mut frame = self.storage.alloc_frame()?;

        frame.push_pdu::<()>(
            Command::bwr(register).into(),
            (),
            Some(payload_length),
            false,
        )?;

        let frame = frame.mark_sendable();

        self.wake_sender();

        frame.await?;

        Ok(())
    }

    // /// Send data to and read data back from the slave devices.
    // ///
    // /// This method allows overriding the minimum data length of the payload.
    // ///
    // /// Returns the frame future that can be `.await`ed for a response, and the EtherCAT frame
    // /// index.
    // ///
    // /// The PDU data length will be the larger of the payload data length and the length override
    // /// (if provided). If a larger **response** than the sent data is desired, set the expected
    // /// response length in `len_override`.
    // ///
    // /// This is useful for e.g. sending a 10 byte PDI with 4 output bytes and 6 input bytes. In this
    // /// case, `data` will be a slice of length `4` containing the outputs to send, and
    // /// `len_override` will be `Some(10)`. This makes the latter 6 bytes available for writing the
    // /// PDU response into.
    // // NOTE: Should be `pub(crate)` but the benchmarks need this internal method so we'll just hide
    // // it instead.
    // #[doc(hidden)]
    // pub fn pdu_send(
    //     &self,
    //     command: Command,
    //     data: impl EtherCrabWireWrite,
    //     len_override: Option<u16>,
    // ) -> Result<
    //     (
    //         impl core::future::Future<Output = Result<ReceivedFrame<'_>, Error>>,
    //         u8,
    //     ),
    //     Error,
    // > {
    //     let mut frame = self.storage.alloc_frame()?;
    //     let frame_idx = frame.frame_index();

    //     let handle = frame.push_pdu::<()>(command, data, len_override, false)?;

    //     Ok((
    //         async move {
    //             let response = frame.mark_sendable().await?;

    //             response.take(handle)
    //         },
    //         frame_idx,
    //     ))

    //     // Done

    //     // ---

    //     // // Length of data to populate in the send buffer
    //     // let send_data_len = data.packed_len() as u16;

    //     // // The length in the header can be set longer to e.g. send PDI outputs, then get PDI
    //     // // inputs in the remaining buffer.
    //     // let total_payload_len: u16 = len_override.unwrap_or(send_data_len).max(send_data_len);

    //     // let mut frame = self.storage.alloc_frame()?;

    //     // let handle = frame.push_pdu(command, data, len_override, false)?;

    //     // // let frame_idx = frame.frame_index();

    //     // // let payload = frame
    //     // //     .buf_mut()
    //     // //     .get_mut(0..usize::from(send_data_len))
    //     // //     .ok_or(Error::Pdu(PduError::TooLong))?;

    //     // // // SAFETY: We ensure the payload length is at least the length of the packed input data
    //     // // // above, as well as the data to be written is not longer than the payload buffer.
    //     // // data.pack_to_slice_unchecked(payload);

    //     // let frame = frame.mark_sendable();

    //     // self.wake_sender();

    //     // todo!()
    //     // // Ok((frame, frame_idx))
    // }
}

#[cfg(test)]
mod tests {
    use crate::{
        error::{Error, PduError},
        fmt,
        pdu_loop::frame_element::{
            created_frame::CreatedFrame, sendable_frame::SendableFrame, FrameElement, FrameState,
        },
        timer_factory::IntoTimeout,
        Command, PduStorage, Reads,
    };
    use core::{future::poll_fn, ops::Deref, pin::pin, task::Poll, time::Duration};
    use futures_lite::Future;
    use smoltcp::wire::{EthernetAddress, EthernetFrame};

    // NOTE: Async required as tokio timers are used internally
    #[test]
    fn timed_out_frame_is_reallocatable() {
        // One 16 byte frame
        static STORAGE: PduStorage<1, { PduStorage::element_size(32) }> = PduStorage::new();
        let (_tx, _rx, pdu_loop) = STORAGE.try_split().unwrap();

        let mut frame = pdu_loop.storage.alloc_frame().expect("Alloc");

        frame
            .push_pdu::<()>(
                Reads::Brd {
                    address: 0,
                    register: 0,
                }
                .into(),
                (),
                Some(16),
                false,
            )
            .expect("Push PDU");

        let fut = frame.mark_sendable();

        let res = cassette::block_on(fut.timeout(Duration::from_secs(0)));

        // Just make sure the read timed out
        assert_eq!(res.unwrap_err(), Error::Timeout);

        let frame = pdu_loop.storage.alloc_frame();

        // We should be able to reuse the frame slot now
        assert!(matches!(frame, Ok(CreatedFrame { .. })));

        // Only one slot so a next alloc should fail
        let f2 = pdu_loop.storage.alloc_frame();

        assert_eq!(f2.unwrap_err(), PduError::SwapState.into());
    }

    #[test]
    fn write_frame() {
        let _ = env_logger::builder().is_test(true).try_init();

        static STORAGE: PduStorage<1, 128> = PduStorage::<1, 128>::new();
        let (_tx, _rx, pdu_loop) = STORAGE.try_split().unwrap();

        let data = [0xaau8, 0xbb, 0xcc];

        let mut frame = pdu_loop.storage.alloc_frame().unwrap();

        let _handle = frame
            .push_pdu::<()>(Command::fpwr(0x5678, 0x1234).into(), data, None, false)
            .expect("Push");

        let frame = frame.finish();

        assert_eq!(
            frame.buf(),
            &[
                0xff, 0xff, 0xff, 0xff, 0xff, 0xff, // Broadcast address
                0x10, 0x10, 0x10, 0x10, 0x10, 0x10, // Master address
                0x88, 0xa4, // EtherCAT ethertype
                0x0f, 0x10, // EtherCAT frame header: type PDU, length 3 (plus header)
                0x05, // Command: FPWR
                0x00, // Frame index 0
                0x78, 0x56, // Slave address,
                0x34, 0x12, // Register address
                0x03, 0x00, // Flags, 3 byte length
                0x00, 0x00, // IRQ
                0xaa, 0xbb, 0xcc, // Our payload
                0x00, 0x00, // Working counter
            ]
        );
    }

    #[test]
    fn single_frame_round_trip() {
        let _ = env_logger::builder().is_test(true).try_init();

        const FRAME_OVERHEAD: usize = 28;

        // 1 frame, up to 128 bytes payload
        let storage = PduStorage::<1, 128>::new();

        let (mut tx, mut rx, pdu_loop) = storage.try_split().unwrap();

        let data = [0xaau8, 0xbb, 0xcc, 0xdd];

        // Using poll_fn so we can manually poll the frame future multiple times
        let poller = poll_fn(|ctx| {
            let mut written_packet = Vec::new();
            written_packet.resize(FRAME_OVERHEAD + data.len(), 0);

            let mut frame = pdu_loop.storage.alloc_frame().expect("Frame alloc");

            let handle = frame
                .push_pdu::<()>(Command::fpwr(0x5678, 0x1234).into(), &data, None, false)
                .expect("Push PDU");

            let mut frame_fut = pin!(frame.mark_sendable());

            // Poll future up to first await point. This gets the frame ready and marks it as
            // sendable so TX can pick it up, but we don't want to wait for the response so we won't
            // poll it again.
            assert!(
                matches!(frame_fut.as_mut().poll(ctx), Poll::Pending),
                "frame fut should be pending"
            );

            let frame = tx.next_sendable_frame().expect("need a frame");

            let send_fut = pin!(async move {
                frame
                    .send_blocking(|bytes| {
                        written_packet.copy_from_slice(bytes);

                        Ok(bytes.len())
                    })
                    .expect("send");

                // Munge fake sent frame into a fake received frame
                {
                    let mut frame = EthernetFrame::new_checked(written_packet).unwrap();
                    frame.set_src_addr(EthernetAddress([0x12, 0x10, 0x10, 0x10, 0x10, 0x10]));
                    frame.into_inner()
                }
            });

            let Poll::Ready(written_packet) = send_fut.poll(ctx) else {
                panic!("no send")
            };

            assert_eq!(written_packet.len(), FRAME_OVERHEAD + data.len());

            // ---

            let result = rx.receive_frame(&written_packet);

            assert_eq!(result, Ok(()));

            // The frame has received a response at this point so should be ready to get the data
            // from
            match frame_fut.poll(ctx) {
                Poll::Ready(Ok(frame)) => {
                    let response = frame.take(handle).expect("Handle");

                    assert_eq!(response.deref(), &data);
                }
                Poll::Ready(other) => panic!("Expected Ready(Ok()), got {:?}", other),
                Poll::Pending => panic!("frame future still pending"),
            }

            // We should only ever be going through this loop once as the number of individual
            // `poll()` calls is calculated.
            Poll::Ready(())
        });

        // Using `cassette` otherwise miri complains about a memory leak inside whichever other
        // `block_on` or `.await` we use.
        cassette::block_on(poller);
    }

    #[test]
    fn write_multiple_frame() {
        static STORAGE: PduStorage<1, 128> = PduStorage::<1, 128>::new();
        let (_tx, _rx, pdu_loop) = STORAGE.try_split().unwrap();

        // ---

        let data = [0xaau8, 0xbb, 0xcc];

        let mut frame = pdu_loop.storage.alloc_frame().unwrap();

        let _handle = frame
            .push_pdu::<()>(Command::fpwr(0x5678, 0x1234).into(), data, None, false)
            .expect("Push PDU");

        let frame = SendableFrame::new(frame.inner());

        // Manually reset frame state so it can be reused.
        unsafe { FrameElement::set_state(frame.inner.frame, FrameState::None) };

        // ---

        let data = [0xaau8, 0xbb];

        let mut frame = pdu_loop.storage.alloc_frame().unwrap();

        let _handle = frame
            .push_pdu::<()>(Command::fpwr(0x6789, 0x1234).into(), data, None, false)
            .expect("Push second PDU");

        let frame = frame.finish();

        // ---

        assert_eq!(
            frame.buf(),
            &[
                0xff, 0xff, 0xff, 0xff, 0xff, 0xff, // Broadcast address
                0x10, 0x10, 0x10, 0x10, 0x10, 0x10, // Master address
                0x88, 0xa4, // EtherCAT ethertype
                0x0e, 0x10, // EtherCAT frame header: type PDU, length 2 (plus header)
                0x05, // Command: FPWR
                0x01, // Frame index 1 (first dropped frame used up index 0)
                0x89, 0x67, // Slave address,
                0x34, 0x12, // Register address
                0x02, 0x00, // Flags, 2 byte length
                0x00, 0x00, // IRQ
                0xaa, 0xbb, // Our payload
                0x00, 0x00, // Working counter
            ]
        );
    }

    #[test]
    fn receive_frame() {
        let _ = env_logger::builder().is_test(true).try_init();

        let ethernet_packet = [
            0xff, 0xff, 0xff, 0xff, 0xff, 0xff, // Broadcast address
            0x12, 0x10, 0x10, 0x10, 0x10, 0x10, // Return to master address
            0x88, 0xa4, // EtherCAT ethertype
            0x10, 0x10, // EtherCAT frame header: type PDU, length 4 (plus header)
            0x05, // Command: FPWR
            0x00, // Frame index 0
            0x89, 0x67, // Slave address,
            0x34, 0x12, // Register address
            0x04, 0x00, // Flags, 4 byte length
            0x00, 0x00, // IRQ
            0xdd, 0xcc, 0xbb, 0xaa, // Our payload, LE
            0x00, 0x00, // Working counter
        ];

        // 1 frame, up to 128 bytes payload
        let storage = PduStorage::<1, 128>::new();

        let (mut tx, mut rx, pdu_loop) = storage.try_split().unwrap();

        let data = 0xAABBCCDDu32;
        let data_bytes = data.to_le_bytes();

        let poller = poll_fn(|ctx| {
            let mut frame = pdu_loop.storage.alloc_frame().unwrap();

            let handle = frame
                .push_pdu::<()>(
                    Command::fpwr(0x6789, 0x1234).into(),
                    &data_bytes,
                    None,
                    false,
                )
                .expect("Push PDU");

            let mut frame_fut = pin!(frame.mark_sendable());

            // Poll future up to first await point. This gets the frame ready and marks it as
            // sendable so TX can pick it up, but we don't want to wait for the response so we won't
            // poll it again.
            assert!(
                matches!(frame_fut.as_mut().poll(ctx), Poll::Pending),
                "frame fut should be pending"
            );

            let frame = tx.next_sendable_frame().expect("need a frame");

            frame.send_blocking(|bytes| Ok(bytes.len())).expect("send");

            // ---

            let result = rx.receive_frame(&ethernet_packet);

            assert_eq!(result, Ok(()));

            // The frame has received a response at this point so should be ready to get the data
            // from
            match frame_fut.poll(ctx) {
                Poll::Ready(Ok(frame)) => {
                    assert_eq!(frame.take(handle).unwrap().deref(), &data_bytes);
                }
                Poll::Ready(other) => panic!("Expected Ready(Ok()), got {:?}", other),
                Poll::Pending => panic!("frame future still pending"),
            }

            // We should only ever be going through this loop once as the number of individual
            // `poll()` calls is calculated.
            Poll::Ready(())
        });

        cassette::block_on(poller);
    }

    // Test the whole TX/RX loop with multiple threads
    #[tokio::test]
    async fn parallel() {
        let _ = env_logger::builder().is_test(true).try_init();
        env_logger::try_init().ok();

        static STORAGE: PduStorage<16, 128> = PduStorage::<16, 128>::new();
        let (mut tx, mut rx, pdu_loop) = STORAGE.try_split().unwrap();

        let tx_rx_task = async move {
            let (s, mut r) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();

            let tx_task = async {
                fmt::info!("Spawn TX task");

                loop {
                    while let Some(frame) = tx.next_sendable_frame() {
                        frame
                            .send_blocking(|bytes| {
                                s.send(bytes.to_vec()).unwrap();

                                Ok(bytes.len())
                            })
                            .unwrap();
                    }

                    futures_lite::future::yield_now().await;
                }
            };

            let rx_task = async {
                fmt::info!("Spawn RX task");

                while let Some(ethernet_frame) = r.recv().await {
                    fmt::trace!("RX task received packet");

                    // Munge fake sent frame into a fake received frame
                    let ethernet_frame = {
                        let mut frame = EthernetFrame::new_checked(ethernet_frame).unwrap();
                        frame.set_src_addr(EthernetAddress([0x12, 0x10, 0x10, 0x10, 0x10, 0x10]));
                        frame.into_inner()
                    };

                    rx.receive_frame(&ethernet_frame).expect("RX");
                }
            };

            futures_lite::future::race(tx_task, rx_task).await;
        };

        tokio::spawn(tx_rx_task);

        for i in 0..32 {
            let data = [0xaa, 0xbb, 0xcc, 0xdd, i];

            fmt::info!("Send PDU {i}");

            let mut frame = pdu_loop.storage.alloc_frame().expect("Frame alloc");

            let handle = frame
                .push_pdu::<()>(Command::fpwr(0x1000, 0x980).into(), data, None, false)
                .expect("Push PDU");

            let result = frame.mark_sendable().await.expect("Future");

            let received_data = result.take(handle).expect("Take");

            assert_eq!(&*received_data, &data);
        }

        fmt::info!("Sent all PDUs");
    }
}
