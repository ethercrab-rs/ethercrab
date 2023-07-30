mod frame_element;
mod frame_header;
mod pdu_flags;
mod pdu_rx;
mod pdu_tx;
// NOTE: Pub so doc links work
pub mod storage;

use crate::{
    command::Command,
    error::{Error, PduError},
    pdu_loop::storage::PduStorageRef,
};

pub use frame_element::received_frame::RxFrameDataBuf;
pub use frame_element::sendable_frame::SendableFrame;
pub use pdu_rx::PduRx;
pub use pdu_tx::PduTx;
pub use storage::PduStorage;

use self::frame_element::received_frame::ReceivedFrame;

pub type PduResponse<T> = (T, u16);

pub trait CheckWorkingCounter<T> {
    fn wkc(self, expected: u16, context: &'static str) -> Result<T, Error>;
}

impl<T> CheckWorkingCounter<T> for PduResponse<T> {
    fn wkc(self, expected: u16, context: &'static str) -> Result<T, Error> {
        if self.1 == expected {
            Ok(self.0)
        } else {
            Err(Error::WorkingCounter {
                expected,
                received: self.1,
                context: Some(context),
            })
        }
    }
}

/// The core of the PDU send/receive machinery.
///
/// This item orchestrates queuing, sending and receiving responses to individual PDUs. It uses a
/// fixed length list of frame slots which are cycled through sequentially to ensure each PDU packet
/// has a unique ID (by using the slot index).
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

unsafe impl<'sto> Send for PduLoop<'sto> {}
unsafe impl<'sto> Sync for PduLoop<'sto> {}

impl<'sto> PduLoop<'sto> {
    /// Create a new PDU loop with the given backing storage.
    pub(in crate::pdu_loop) const fn new(storage: PduStorageRef<'sto>) -> Self {
        assert!(storage.num_frames <= u8::MAX as usize);

        Self { storage }
    }

    pub(crate) fn max_frame_data(&self) -> usize {
        self.storage.frame_data_len
    }

    /// Read data back from one or more slave devices.
    pub async fn pdu_tx_readonly(
        &self,
        command: Command,
        data_length: u16,
    ) -> Result<ReceivedFrame<'_>, Error> {
        let frame = self.storage.alloc_frame(command, data_length)?;

        let frame = frame.mark_sendable();

        self.wake_sender();

        let res = frame.await?;

        Ok(res)
    }

    /// Tell the packet sender there is data ready to send.
    fn wake_sender(&self) {
        let waker = self.storage.tx_waker.read();

        if let Some(waker) = &*waker {
            waker.wake_by_ref()
        }
    }

    /// Broadcast (BWR) a packet full of zeroes, up to `max_data_length`.
    pub async fn pdu_broadcast_zeros(
        &self,
        register: u16,
        payload_length: u16,
    ) -> Result<ReceivedFrame<'_>, Error> {
        let frame = self.storage.alloc_frame(
            Command::Bwr {
                address: 0,
                register,
            },
            payload_length,
        )?;

        let frame = frame.mark_sendable();

        self.wake_sender();

        frame.await
    }

    /// Send data to and read data back from multiple slaves.
    ///
    /// Unlike [`pdu_tx_readwrite`](crate::pdu_loop::PduLoop::pdu_tx_readwrite), this method allows
    /// overriding the minimum data length of the payload.
    ///
    /// The PDU data length will be the larger of `send_data.len()` and `data_length`. If a larger
    /// response than `send_data` is desired, set the expected response length in `data_length`.
    pub async fn pdu_tx_readwrite_len(
        &self,
        command: Command,
        send_data: &[u8],
        data_length: u16,
    ) -> Result<ReceivedFrame<'_>, Error> {
        let send_data_len = send_data.len();
        let payload_length = u16::try_from(send_data.len())?.max(data_length);

        let mut frame = self.storage.alloc_frame(command, data_length)?;

        let payload = frame
            .buf_mut()
            .get_mut(0..usize::from(payload_length))
            .ok_or(Error::Pdu(PduError::TooLong))?;

        payload[0..send_data_len].copy_from_slice(send_data);

        let frame = frame.mark_sendable();

        self.wake_sender();

        frame.await
    }

    /// Send data to and read data back from multiple slaves.
    pub async fn pdu_tx_readwrite<'a>(
        &'a self,
        command: Command,
        send_data: &[u8],
    ) -> Result<ReceivedFrame<'_>, Error> {
        self.pdu_tx_readwrite_len(command, send_data, send_data.len().try_into()?)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::{storage::PduStorage, *};
    use crate::pdu_loop::frame_element::{
        sendable_frame::SendableFrame, FrameBox, FrameElement, FrameState,
    };
    use core::{future::poll_fn, marker::PhantomData, ops::Deref, pin::pin, task::Poll};
    use futures_lite::Future;
    use smoltcp::wire::{EthernetAddress, EthernetFrame};

    #[test]
    fn write_frame() {
        static STORAGE: PduStorage<1, 128> = PduStorage::<1, 128>::new();
        let (_tx, _rx, pdu_loop) = STORAGE.try_split().unwrap();

        let data = [0xaau8, 0xbb, 0xcc];

        let mut frame = pdu_loop
            .storage
            .alloc_frame(
                Command::Fpwr {
                    address: 0x5678,
                    register: 0x1234,
                },
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
                Command::Fpwr {
                    address: 0x5678,
                    register: 0x1234,
                },
                &data,
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

                        Ok(())
                    })
                    .await
                    .expect("send");

                // Munge fake sent frame into a fake received frame
                let written_packet = {
                    let mut frame = EthernetFrame::new_checked(written_packet).unwrap();
                    frame.set_src_addr(EthernetAddress([0x12, 0x10, 0x10, 0x10, 0x10, 0x10]));
                    frame.into_inner()
                };

                written_packet
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
                Command::Fpwr {
                    address: 0x5678,
                    register: 0x1234,
                },
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
                Command::Fpwr {
                    address: 0x6789,
                    register: 0x1234,
                },
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
                log::info!("Spawn TX task");

                let mut packet_buf = [0u8; 1536];

                loop {
                    while let Some(frame) = tx.next_sendable_frame() {
                        frame
                            .send(&mut packet_buf, |bytes| async {
                                s.send(bytes.to_vec()).unwrap();

                                Ok(())
                            })
                            .await
                            .unwrap();
                    }

                    futures_lite::future::yield_now().await;
                }
            };

            let rx_task = async {
                log::info!("Spawn RX task");

                while let Some(ethernet_frame) = r.recv().await {
                    log::trace!("RX task received packet");

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

        for i in 0..64 {
            let data = [0xaa, 0xbb, 0xcc, 0xdd, i];

            log::info!("Send PDU {i}");

            let result = pdu_loop
                .pdu_tx_readwrite(
                    Command::Fpwr {
                        address: 0x1000,
                        register: 0x0980,
                    },
                    &data,
                )
                .await
                .unwrap()
                .wkc(0, "testing")
                .unwrap();

            assert_eq!(&*result, &data);
        }

        log::info!("Sent all PDUs");
    }
}
