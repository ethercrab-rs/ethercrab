mod frame_element;
mod frame_header;
mod pdu_flags;
mod pdu_rx;
mod pdu_tx;
// NOTE: Pub so doc links work
pub mod storage;

use crate::{
    client_config::RetryBehaviour,
    command::Command,
    error::{Error, PduError},
    fmt,
    pdu_loop::storage::PduStorageRef,
    Reads, Writes,
};
use core::time::Duration;

use ethercrab_wire::EtherCrabWireWrite;
pub use frame_element::received_frame::RxFrameDataBuf;
pub use frame_element::sendable_frame::SendableFrame;
pub use pdu_rx::PduRx;
pub use pdu_tx::PduTx;
pub use storage::PduStorage;

use self::frame_element::received_frame::ReceivedFrame;

pub type PduResponse<T> = (T, u16);

pub trait CheckWorkingCounter<T> {
    fn wkc(self, expected: u16) -> Result<T, Error>;
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
    storage: PduStorageRef<'sto>,
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
    fn wake_sender(&self) {
        self.storage.tx_waker.wake();
    }

    /// Broadcast (BWR) a packet full of zeroes, up to `payload_length`.
    pub(crate) async fn pdu_broadcast_zeros(
        &self,
        register: u16,
        payload_length: u16,
    ) -> Result<ReceivedFrame<'_>, Error> {
        let frame = self
            .storage
            .alloc_frame(Command::Write(Command::bwr(register)), payload_length)?;

        let frame = frame.mark_sendable();

        self.wake_sender();

        frame.await
    }

    /// Send data to and read data back from the slave devices.
    ///
    /// Unlike [`pdu_tx_readwrite`](crate::pdu_loop::PduLoop::pdu_tx_readwrite), this method allows
    /// overriding the minimum data length of the payload.
    ///
    /// The PDU data length will be the larger of `send_data.len()` and `data_length`. If a larger
    /// response than `send_data` is desired, set the expected response length in `data_length`.
    ///
    /// This is useful for e.g. sending a 10 byte PDI with 4 output bytes and 6 input bytes. In this
    /// case, `send_data` will be a slice of length `4` containing the outputs to send, and
    /// `data_length` will be `10`. This makes the latter 6 bytes available for writing the PDU
    /// response into.
    pub(crate) async fn pdu_tx_readwrite_len(
        &self,
        command: Command,
        send_data: impl EtherCrabWireWrite,
        data_length: u16,
        timeout: Duration,
        retry_behaviour: RetryBehaviour,
    ) -> Result<ReceivedFrame<'_>, Error> {
        for _ in 0..retry_behaviour.loop_counts() {
            let payload_length = u16::try_from(send_data.packed_len())?.max(data_length);

            let mut frame = self.storage.alloc_frame(command, data_length)?;

            let frame_idx = frame.index();

            let payload = frame
                .buf_mut()
                .get_mut(0..usize::from(payload_length))
                .ok_or(Error::Pdu(PduError::TooLong))?;

            // SAFETY: We ensure the payload length is at least the length of the packed input data
            // above, as well as the data to be written is not longer than the payload buffer.
            send_data.pack_to_slice_unchecked(payload);

            let frame = frame.mark_sendable();

            self.wake_sender();

            match crate::timer_factory::timeout(timeout, frame).await {
                Ok(result) => return Ok(result),
                Err(Error::Timeout) => {
                    fmt::error!("Frame {} timed out", frame_idx);

                    // NOTE: The `Drop` impl of `ReceiveFrameFut` frees the frame by setting its
                    // state to `None`, ready for reuse.
                }
                Err(e) => return Err(e),
            }
        }

        Err(Error::Timeout)
    }

    /// Send data to and read data back from multiple slaves.
    // NOTE: Should be `pub(crate)` but the benchmarks need this internal method so we'll just hide
    // it instead.
    #[doc(hidden)]
    pub async fn pdu_tx_readwrite<'a>(
        &'a self,
        command: Writes,
        send_data: impl EtherCrabWireWrite,
        timeout: Duration,
        retry_behaviour: RetryBehaviour,
    ) -> Result<ReceivedFrame<'_>, Error> {
        let len = send_data.packed_len() as u16;

        self.pdu_tx_readwrite_len(
            Command::Write(command),
            send_data,
            len,
            timeout,
            retry_behaviour,
        )
        .await
    }

    /// Send a PDU to read data back from one or more slave devices.
    pub(crate) async fn pdu_tx_readonly(
        &self,
        command: Reads,
        data_length: u16,
        timeout: Duration,
        retry_behaviour: RetryBehaviour,
    ) -> Result<ReceivedFrame<'_>, Error> {
        self.pdu_tx_readwrite_len(
            Command::Read(command),
            &[],
            data_length,
            timeout,
            retry_behaviour,
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::{storage::PduStorage, *};
    use crate::{
        fmt,
        pdu_loop::frame_element::{
            created_frame::CreatedFrame, sendable_frame::SendableFrame, FrameBox, FrameElement,
            FrameState,
        },
    };
    use core::{future::poll_fn, marker::PhantomData, ops::Deref, pin::pin, task::Poll};
    use futures_lite::Future;
    use smoltcp::wire::{EthernetAddress, EthernetFrame};

    #[tokio::test]
    // MIRI doesn't like this test as it leaks memory
    #[cfg_attr(miri, ignore)]
    async fn timed_out_frame_is_reallocatable() {
        // One 16 byte frame
        static STORAGE: PduStorage<1, 16> = PduStorage::new();
        let (_tx, _rx, pdu_loop) = STORAGE.try_split().unwrap();

        let send_result = pdu_loop
            .pdu_tx_readonly(
                Reads::Brd {
                    address: 0,
                    register: 0,
                },
                16,
                Duration::from_secs(0),
                RetryBehaviour::None,
            )
            .await;

        // Just make sure the read timed out
        assert_eq!(send_result.unwrap_err(), Error::Timeout);

        // We should be able to reuse the frame slot now
        assert!(matches!(
            pdu_loop.storage.alloc_frame(
                Command::Read(Reads::Lrd {
                    address: 0x1234_5678,
                }),
                16,
            ),
            Ok(CreatedFrame { .. })
        ));
    }

    #[test]
    fn write_frame() {
        static STORAGE: PduStorage<1, 128> = PduStorage::<1, 128>::new();
        let (_tx, _rx, pdu_loop) = STORAGE.try_split().unwrap();

        let data = [0xaau8, 0xbb, 0xcc];

        let mut frame = pdu_loop
            .storage
            .alloc_frame(
                Command::Write(Command::fpwr(0x5678, 0x1234)),
                data.len() as u16,
            )
            .unwrap();

        frame.buf_mut().copy_from_slice(&data);

        let frame = SendableFrame::new(FrameBox {
            frame: frame.inner.frame,
            _lifetime: PhantomData,
        });

        let mut packet_buf = [0u8; 1536];

        let packet = frame.write_ethernet_packet(&mut packet_buf).unwrap();

        assert_eq!(
            packet,
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
    // MIRI fails this test with `unsupported operation: can't execute syscall with ID 291`.
    #[cfg_attr(miri, ignore)]
    fn single_frame_round_trip() {
        let _ = env_logger::builder().is_test(true).try_init();

        const FRAME_OVERHEAD: usize = 28;

        // 1 frame, up to 128 bytes payload
        let storage = PduStorage::<1, 128>::new();

        let (mut tx, mut rx, pdu_loop) = storage.try_split().unwrap();

        let data = [0xaau8, 0xbb, 0xcc, 0xdd];

        let poller = poll_fn(|ctx| {
            let mut written_packet = Vec::new();
            written_packet.resize(FRAME_OVERHEAD + data.len(), 0);

            let mut frame_fut = pin!(pdu_loop.pdu_tx_readwrite(
                Command::fpwr(0x5678, 0x1234),
                &data,
                Duration::from_secs(1),
                RetryBehaviour::None
            ));

            // Poll future up to first await point. This gets the frame ready and marks it as
            // sendable so TX can pick it up, but we don't want to wait for the response so we won't
            // poll it again.
            assert!(
                matches!(frame_fut.as_mut().poll(ctx), Poll::Pending),
                "frame fut should be pending"
            );

            let mut packet_buf = [0u8; 1536];

            let frame = tx.next_sendable_frame().expect("need a frame");

            let send_fut = pin!(async move {
                frame
                    .send(&mut packet_buf, |bytes| async {
                        written_packet.copy_from_slice(bytes);

                        Ok(bytes.len())
                    })
                    .await
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

            assert!(result.is_ok());

            // The frame has received a response at this point so should be ready to get the data
            // from
            match frame_fut.poll(ctx) {
                Poll::Ready(Ok(frame)) => {
                    assert_eq!(frame.into_data().0.deref(), &data);
                }
                Poll::Ready(other) => panic!("Expected Ready(Ok()), got {:?}", other),
                Poll::Pending => panic!("frame future still pending"),
            }

            // We should only ever be going through this loop once as the number of individual
            // `poll()` calls is calculated.
            Poll::Ready(())
        });

        smol::block_on(poller);
    }

    #[test]
    fn write_multiple_frame() {
        static STORAGE: PduStorage<1, 128> = PduStorage::<1, 128>::new();
        let (_tx, _rx, pdu_loop) = STORAGE.try_split().unwrap();

        let mut packet_buf = [0u8; 1536];

        // ---

        let data = [0xaau8, 0xbb, 0xcc];

        let mut frame = pdu_loop
            .storage
            .alloc_frame(
                Command::Write(Command::fpwr(0x5678, 0x1234)),
                data.len() as u16,
            )
            .unwrap();

        frame.buf_mut().copy_from_slice(&data);

        let frame = SendableFrame::new(FrameBox {
            frame: frame.inner.frame,
            _lifetime: PhantomData,
        });

        let _packet_1 = frame.write_ethernet_packet(&mut packet_buf).unwrap();

        // Manually reset frame state so it can be reused.
        unsafe { FrameElement::set_state(frame.inner.frame, FrameState::None) };

        // ---

        let data = [0xaau8, 0xbb];

        let mut frame = pdu_loop
            .storage
            .alloc_frame(
                Command::Write(Command::fpwr(0x6789, 0x1234)),
                data.len() as u16,
            )
            .unwrap();

        frame.buf_mut().copy_from_slice(&data);

        let frame = SendableFrame::new(FrameBox {
            frame: frame.inner.frame,
            _lifetime: PhantomData,
        });

        let packet_2 = frame.write_ethernet_packet(&mut packet_buf).unwrap();

        // ---

        assert_eq!(
            packet_2,
            &[
                0xff, 0xff, 0xff, 0xff, 0xff, 0xff, // Broadcast address
                0x10, 0x10, 0x10, 0x10, 0x10, 0x10, // Master address
                0x88, 0xa4, // EtherCAT ethertype
                0x0e, 0x10, // EtherCAT frame header: type PDU, length 2 (plus header)
                0x05, // Command: FPWR
                0x00, // Frame index 0
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
    // MIRI fails this test with `unsupported operation: can't execute syscall with ID 291`.
    #[cfg_attr(miri, ignore)]
    fn receive_frame() {
        // let _ = env_logger::builder().is_test(true).try_init();

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
            let mut frame_fut = pin!(pdu_loop.pdu_tx_readwrite(
                Command::fpwr(0x6789, 0x1234),
                &data_bytes,
                Duration::from_secs(1),
                RetryBehaviour::None
            ));

            // Poll future up to first await point. This gets the frame ready and marks it as
            // sendable so TX can pick it up, but we don't want to wait for the response so we won't
            // poll it again.
            assert!(
                matches!(frame_fut.as_mut().poll(ctx), Poll::Pending),
                "frame fut should be pending"
            );

            let mut packet_buf = [0u8; 1536];

            let frame = tx.next_sendable_frame().expect("need a frame");

            let send_fut = pin!(async move {
                frame
                    .send(&mut packet_buf, |bytes| async {
                        // written_packet.copy_from_slice(bytes);

                        Ok(bytes.len())
                    })
                    .await
                    .expect("send");
            });

            let Poll::Ready(_) = send_fut.poll(ctx) else {
                panic!("no send")
            };

            // ---

            let result = rx.receive_frame(&ethernet_packet);

            assert_eq!(result, Ok(()));

            // The frame has received a response at this point so should be ready to get the data
            // from
            match frame_fut.poll(ctx) {
                Poll::Ready(Ok(frame)) => {
                    assert_eq!(frame.into_data().0.deref(), &data_bytes);
                }
                Poll::Ready(other) => panic!("Expected Ready(Ok()), got {:?}", other),
                Poll::Pending => panic!("frame future still pending"),
            }

            // We should only ever be going through this loop once as the number of individual
            // `poll()` calls is calculated.
            Poll::Ready(())
        });

        smol::block_on(poller);
    }

    // Test the whole TX/RX loop with multiple threads
    #[tokio::test]
    async fn parallel() {
        // let _ = env_logger::builder().is_test(true).try_init();
        // env_logger::try_init().ok();

        static STORAGE: PduStorage<16, 128> = PduStorage::<16, 128>::new();
        let (mut tx, mut rx, pdu_loop) = STORAGE.try_split().unwrap();

        let tx_rx_task = async move {
            let (s, mut r) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();

            let tx_task = async {
                fmt::info!("Spawn TX task");

                let mut packet_buf = [0u8; 1536];

                loop {
                    while let Some(frame) = tx.next_sendable_frame() {
                        frame
                            .send(&mut packet_buf, |bytes| async {
                                s.send(bytes.to_vec()).unwrap();

                                Ok(bytes.len())
                            })
                            .await
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

            embassy_futures::select::select(tx_task, rx_task).await;
        };

        tokio::spawn(tx_rx_task);

        for i in 0..32 {
            let data = [0xaa, 0xbb, 0xcc, 0xdd, i];

            fmt::info!("Send PDU {i}");

            let result = pdu_loop
                .pdu_tx_readwrite(
                    Command::fpwr(0x1000, 0x980),
                    &data,
                    Duration::from_secs(1),
                    RetryBehaviour::None,
                )
                .await
                .unwrap()
                .wkc(0)
                .unwrap();

            assert_eq!(&*result, &data);
        }

        fmt::info!("Sent all PDUs");
    }
}
