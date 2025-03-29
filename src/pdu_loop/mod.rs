mod frame_element;
mod frame_header;
mod pdu_flags;
mod pdu_header;
mod pdu_rx;
mod pdu_tx;
// NOTE: Pub so doc links work
pub mod storage;

use crate::{command::Command, error::Error, pdu_loop::storage::PduStorageRef};
use core::{sync::atomic::Ordering, time::Duration};
pub use pdu_rx::PduRx;
// NOTE: Allowing unused because `ReceiveAction` isn't used when `xdp` is not enabled.
#[allow(unused)]
pub use pdu_rx::ReceiveAction;
pub use pdu_tx::PduTx;
pub use storage::PduStorage;

pub(crate) use self::frame_element::created_frame::CreatedFrame;
#[cfg(test)]
pub(crate) use frame_element::received_frame::ReceivedFrame;
pub(crate) use frame_element::received_frame::ReceivedPdu;
pub use frame_element::sendable_frame::SendableFrame;

/// The core EtherCrab network communications driver.
///
// TODO: Update the following docs. The current text is out of date.
// This item orchestrates queuing, sending and receiving responses to individual PDUs. It uses a
// fixed length list of frame slots which are cycled through sequentially to ensure each PDU packet
// has a unique ID (by using the slot index).
//
// Use [`PduTx`] and [`PduRx`] to integrate EtherCrab into network drivers.
//
// # High level overview
//
// <img alt="High level overview of the PDU loop send/receive process" style="background: white" src="https://mermaid.ink/svg/pako:eNplkcFuwjAMhl8lyrkT9x64jOsmBEzqoRcrcUtEm2SJA0KId5_dFmlbc3GUfP79235oEyzqWmf8LugN7hz0CcbWKz4REjnjInhSBoZBQVbvHJ3vleStqf3uSyAJQwhxDZyaQyPEqdnwhc4Jwa6pT6RbSJf5Y6r8tt2Kaq2O6C0XH0fwdmOBYIakojCizxBBj6rjRrBSN7ggv08xzfTkQvCl0CIbwVyQFAVl8eoM5pleoF_6BzTorrgk_NOcbO4hZVQJcww-v0x0hUrCv4alOxGcQSXzuIuDklFXesQ0grO8oIektZrOOGKra75a4Anp1j-Zg0LhePdG15QKVrpEHs1rmbruYGATGq2jkD7mjU-Lf_4AMq-oMQ" />
//
// Source (MermaidJS)
//
// ```mermaid
// sequenceDiagram
//     participant call as Calling code
//     participant PDU as PDU loop
//     participant TXRX as TX/RX thread
//     participant Network
//     call ->> PDU: Send command/data
//     PDU ->> TXRX: Stage frame, wake TX waker
//     TXRX ->> Network: Send packet to devices
//     Network ->> TXRX: Receive packet
//     TXRX ->> PDU: Parse response, wake future
//     PDU ->> call: Response ready to use
// ```
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

    /// Reset all internal state so the PDU loop can be reused.
    ///
    /// This is useful when calling [`MainDevice::release`](crate::MainDevice::release) or
    /// [`MainDevice::release_all`](crate::MainDevice::release_all) to reset the PDU loop ready for
    /// another `MainDevice` to be created.
    pub fn reset(&mut self) {
        self.storage.reset();
    }

    /// Reset all internal state and tell the TX/RX task to stop.
    pub(crate) fn reset_all(&mut self) {
        self.storage.exit_flag.store(true, Ordering::Release);
        self.wake_sender();

        self.storage.reset();
    }

    #[cfg(test)]
    pub(crate) fn test_only_storage_ref(&self) -> &PduStorageRef<'sto> {
        &self.storage
    }

    pub(crate) const fn max_frame_data(&self) -> usize {
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
        timeout: Duration,
        retries: usize,
    ) -> Result<(), Error> {
        let mut frame = self.storage.alloc_frame()?;

        frame.push_pdu(Command::bwr(register).into(), (), Some(payload_length))?;

        let frame = frame.mark_sendable(self, timeout, retries);

        self.wake_sender();

        frame.await?;

        Ok(())
    }

    pub(crate) fn alloc_frame(&self) -> Result<CreatedFrame<'sto>, Error> {
        self.storage.alloc_frame()
    }
}

#[cfg(test)]
mod tests {
    use crate::ethernet::{EthernetAddress, EthernetFrame};
    use crate::pdu_loop::frame_element::created_frame::PduResponseHandle;
    use crate::pdu_loop::frame_element::received_frame::ReceivedFrame;
    use crate::pdu_loop::frame_header::EthercatFrameHeader;
    use crate::{
        Command, PduStorage, Reads,
        error::{Error, PduError},
        fmt,
        pdu_loop::frame_element::created_frame::CreatedFrame,
        timer_factory::IntoTimeout,
    };
    use cassette::Cassette;
    use core::{future::poll_fn, ops::Deref, pin::pin, task::Poll, time::Duration};
    use futures_lite::Future;
    use std::{sync::Arc, thread};

    #[test]
    fn timed_out_frame_is_reallocatable() {
        // One 16 byte frame
        static STORAGE: PduStorage<1, { PduStorage::element_size(32) }> = PduStorage::new();
        let (_tx, _rx, pdu_loop) = STORAGE.try_split().unwrap();

        let mut frame = pdu_loop.storage.alloc_frame().expect("Alloc");

        frame
            .push_pdu(
                Reads::Brd {
                    address: 0,
                    register: 0,
                }
                .into(),
                (),
                Some(16),
            )
            .expect("Push PDU");

        let fut = frame.mark_sendable(&pdu_loop, Duration::MAX, usize::MAX);

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
        crate::test_logger();

        static STORAGE: PduStorage<1, 128> = PduStorage::<1, 128>::new();
        let (_tx, _rx, pdu_loop) = STORAGE.try_split().unwrap();

        let data = [0xaau8, 0xbb, 0xcc];

        let mut frame = pdu_loop.storage.alloc_frame().unwrap();

        let _handle = frame
            .push_pdu(Command::fpwr(0x5678, 0x1234).into(), data, None)
            .expect("Push");

        let frame = frame.mark_sendable(&pdu_loop, Duration::MAX, usize::MAX);

        assert_eq!(
            frame.buf(),
            &[
                0xff, 0xff, 0xff, 0xff, 0xff, 0xff, // Broadcast address
                0x10, 0x10, 0x10, 0x10, 0x10, 0x10, // Master address
                0x88, 0xa4, // EtherCAT ethertype
                0x0f, 0x10, // EtherCAT frame header: type PDU, length 3 (plus header)
                0x05, // Command: FPWR
                0x00, // Frame index 0
                0x78, 0x56, // SubDevice address,
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
        crate::test_logger();

        const FRAME_OVERHEAD: usize = 28;

        // 1 frame, up to 128 bytes payload
        let storage = PduStorage::<1, 128>::new();

        let (mut tx, mut rx, pdu_loop) = storage.try_split().unwrap();

        let data = [0xaau8, 0xbb, 0xcc, 0xdd];

        // Using poll_fn so we can manually poll the frame future multiple times
        let poller = poll_fn(|ctx| {
            let mut written_packet = vec![0; FRAME_OVERHEAD + data.len()];

            let mut frame = pdu_loop.storage.alloc_frame().expect("Frame alloc");

            let handle = frame
                .push_pdu(Command::fpwr(0x5678, 0x1234).into(), data, None)
                .expect("Push PDU");

            let mut frame_fut = pin!(frame.mark_sendable(&pdu_loop, Duration::MAX, usize::MAX));

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

            assert_eq!(result, Ok(crate::ReceiveAction::Processed));

            // The frame has received a response at this point so should be ready to get the data
            // from
            match frame_fut.poll(ctx) {
                Poll::Ready(Ok(frame)) => {
                    let response = frame.first_pdu(handle).expect("Handle");

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
            .push_pdu(Command::fpwr(0x5678, 0x1234).into(), data, None)
            .expect("Push PDU");

        // Drop frame future to reset its state to `FrameState::None`
        drop(frame.mark_sendable(&pdu_loop, Duration::MAX, usize::MAX));

        // ---

        let data = [0xaau8, 0xbb];

        let mut frame = pdu_loop.storage.alloc_frame().unwrap();

        let _handle = frame
            .push_pdu(Command::fpwr(0x6789, 0x1234).into(), data, None)
            .expect("Push second PDU");

        let frame = frame.mark_sendable(&pdu_loop, Duration::MAX, usize::MAX);

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
                0x89, 0x67, // SubDevice address,
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
        crate::test_logger();

        let ethernet_packet = [
            0xff, 0xff, 0xff, 0xff, 0xff, 0xff, // Broadcast address
            0x12, 0x10, 0x10, 0x10, 0x10, 0x10, // Return to master address
            0x88, 0xa4, // EtherCAT ethertype
            0x10, 0x10, // EtherCAT frame header: type PDU, length 4 (plus header)
            0x05, // Command: FPWR
            0x00, // Frame index 0
            0x89, 0x67, // SubDevice address,
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
                .push_pdu(Command::fpwr(0x6789, 0x1234).into(), data_bytes, None)
                .expect("Push PDU");

            let mut frame_fut = pin!(frame.mark_sendable(&pdu_loop, Duration::MAX, usize::MAX));

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

            assert_eq!(result, Ok(crate::ReceiveAction::Processed));

            // The frame has received a response at this point so should be ready to get the data
            // from
            match frame_fut.poll(ctx) {
                Poll::Ready(Ok(frame)) => {
                    assert_eq!(frame.first_pdu(handle).unwrap().deref(), &data_bytes);
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

    // Frames whos response is received from the network and ready for use before the first poll
    // should still complete, instead of failing with a `NoWaker` error.
    //
    // Related to <https://github.com/ethercrab-rs/ethercrab/discussions/259>
    #[test]
    fn receive_frame_before_first_poll() {
        crate::test_logger();

        let ethernet_packet = [
            0xff, 0xff, 0xff, 0xff, 0xff, 0xff, // Broadcast address
            0x12, 0x10, 0x10, 0x10, 0x10, 0x10, // Return to master address
            0x88, 0xa4, // EtherCAT ethertype
            0x10, 0x10, // EtherCAT frame header: type PDU, length 4 (plus header)
            0x05, // Command: FPWR
            0x00, // Frame index 0
            0x89, 0x67, // SubDevice address,
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

        let mut frame = pdu_loop.storage.alloc_frame().unwrap();

        frame
            .push_pdu(Command::fpwr(0x6789, 0x1234).into(), data_bytes, None)
            .expect("Push PDU");

        let frame_fut = pin!(frame.mark_sendable(&pdu_loop, Duration::MAX, usize::MAX));

        let frame = tx.next_sendable_frame().expect("need a frame");

        frame.send_blocking(|bytes| Ok(bytes.len())).expect("send");

        // ---

        let result = rx.receive_frame(&ethernet_packet);

        assert_eq!(result, Ok(crate::ReceiveAction::Processed));

        let mut frame_fut = Cassette::new(frame_fut);

        // Poll future AFTER frame is received
        let frame_poll = frame_fut.poll_on();

        assert!(
            matches!(frame_poll, Some(Ok(ReceivedFrame { .. }))),
            "frame future should have completed"
        );

        // TODO: Check future result. Should be Poll::Ready
    }

    #[tokio::test]
    async fn tokio_spawn() {
        crate::test_logger();

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
                .push_pdu(Command::fpwr(0x1000, 0x980).into(), data, None)
                .expect("Push PDU");

            let result = frame
                .mark_sendable(&pdu_loop, Duration::MAX, usize::MAX)
                .await
                .expect("Future");

            let received_data = result.first_pdu(handle).expect("Take");

            assert_eq!(&*received_data, &data);
        }

        fmt::info!("Sent all PDUs");
    }

    #[test]
    fn multiple_threads() {
        crate::test_logger();

        const MAX_SUBDEVICES: usize = 16;

        static STORAGE: PduStorage<MAX_SUBDEVICES, 128> = PduStorage::<MAX_SUBDEVICES, 128>::new();
        let (mut tx, mut rx, pdu_loop) = STORAGE.try_split().unwrap();

        let (sent, received) = thread::scope(|s| {
            let (mock_net_tx, mock_net_rx) = std::sync::mpsc::sync_channel::<Vec<u8>>(16);

            let sent = s.spawn(move || {
                fmt::info!("Spawn TX task");

                let mut sent = 0;

                loop {
                    while let Some(frame) = tx.next_sendable_frame() {
                        fmt::info!("Sendable frame");

                        frame
                            .send_blocking(|bytes| {
                                mock_net_tx.send(bytes.to_vec()).unwrap();

                                sent += 1;

                                Ok(bytes.len())
                            })
                            .unwrap();

                        thread::yield_now();
                    }

                    thread::sleep(Duration::from_millis(1));

                    if sent == MAX_SUBDEVICES {
                        break sent;
                    }
                }
            });

            let received = s.spawn(move || {
                fmt::info!("Spawn RX task");

                let mut received = 0;

                while let Ok(ethernet_frame) = mock_net_rx.recv() {
                    fmt::info!("RX task received packet");

                    // Let frame settle for a mo
                    thread::sleep(Duration::from_millis(1));

                    // Munge fake sent frame into a fake received frame
                    let ethernet_frame = {
                        let mut frame = EthernetFrame::new_checked(ethernet_frame).unwrap();
                        frame.set_src_addr(EthernetAddress([0x12, 0x10, 0x10, 0x10, 0x10, 0x10]));
                        frame.into_inner()
                    };

                    while rx.receive_frame(&ethernet_frame).is_err() {}

                    thread::yield_now();

                    received += 1;

                    if received == MAX_SUBDEVICES {
                        break;
                    }
                }

                received
            });

            let pdu_loop = Arc::new(pdu_loop);

            for i in 0..MAX_SUBDEVICES {
                let pdu_loop = pdu_loop.clone();

                s.spawn(move || {
                    let data = [0xaau8, 0xbb, 0xcc, 0xdd, i as u8];

                    fmt::info!("Send PDU {i}");

                    let mut frame = pdu_loop.storage.alloc_frame().expect("Frame alloc");

                    let handle = frame
                        .push_pdu(Command::fpwr(0x1000, 0x980).into(), data, None)
                        .expect("Push PDU");

                    let frame = pin!(frame.mark_sendable(&pdu_loop, Duration::MAX, usize::MAX));

                    let mut x = Cassette::new(frame);

                    let result = loop {
                        if let Some(res) = x.poll_on() {
                            break res;
                        }

                        thread::sleep(Duration::from_millis(1));

                        thread::yield_now();
                    }
                    .expect("Future");

                    let received_data = result.first_pdu(handle).expect("Take");

                    assert_eq!(&*received_data, &data);
                });
            }

            (sent.join().unwrap(), received.join().unwrap())
        });

        assert_eq!(sent, received);
        assert_eq!(sent, MAX_SUBDEVICES);

        fmt::info!("Sent all PDUs");
    }

    #[test]
    fn split_pdi() {
        crate::test_logger();

        const DATA: usize = 48;

        // 8 frames, 32 bytes each
        static STORAGE: PduStorage<8, DATA> = PduStorage::new();
        let (_tx, _rx, pdu_loop) = STORAGE.try_split().unwrap();

        let pdi = [0xaau8; 64];
        let mut remaining = &pdi[..];

        let (sent, _handle) = {
            let mut frame = pdu_loop.alloc_frame().expect("No frame");

            let res = frame.push_pdu_slice_rest(Command::Nop, remaining);

            let expected_pushed_bytes = DATA
                - EthernetFrame::<&[u8]>::header_len()
                - EthercatFrameHeader::header_len()
                - CreatedFrame::PDU_OVERHEAD_BYTES;

            assert_eq!(expected_pushed_bytes, 20);

            assert_eq!(
                res,
                Ok(Some((
                    expected_pushed_bytes,
                    PduResponseHandle {
                        index_in_frame: 0,
                        pdu_idx: 0,
                        command_code: 0,
                        // We've used as much of the frame as possible
                        alloc_size: pdu_loop.max_frame_data()
                            - EthernetFrame::<&[u8]>::header_len()
                            - EthercatFrameHeader::header_len()
                    }
                )))
            );

            res.unwrap().unwrap()
        };

        remaining = &remaining[sent..];

        assert_eq!(remaining.len(), 44);

        let (sent, _handle) = {
            let mut frame = pdu_loop.alloc_frame().expect("No frame");

            let res = frame.push_pdu_slice_rest(Command::Nop, remaining);

            let expected_pushed_bytes = DATA
                - EthernetFrame::<&[u8]>::header_len()
                - EthercatFrameHeader::header_len()
                - CreatedFrame::PDU_OVERHEAD_BYTES;

            assert_eq!(expected_pushed_bytes, 20);

            assert_eq!(
                res,
                Ok(Some((
                    expected_pushed_bytes,
                    PduResponseHandle {
                        index_in_frame: 0,
                        pdu_idx: 1,
                        command_code: 0,
                        // We've used as much of the frame as possible
                        alloc_size: pdu_loop.max_frame_data()
                            - EthernetFrame::<&[u8]>::header_len()
                            - EthercatFrameHeader::header_len()
                    }
                )))
            );

            res.unwrap().unwrap()
        };

        remaining = &remaining[sent..];

        assert_eq!(remaining.len(), 24);

        let (sent, _handle) = {
            let mut frame = pdu_loop.alloc_frame().expect("No frame");

            let res = frame.push_pdu_slice_rest(Command::Nop, remaining);

            let expected_pushed_bytes = DATA
                - EthernetFrame::<&[u8]>::header_len()
                - EthercatFrameHeader::header_len()
                - CreatedFrame::PDU_OVERHEAD_BYTES;

            assert_eq!(expected_pushed_bytes, 20);

            assert_eq!(
                res,
                Ok(Some((
                    expected_pushed_bytes,
                    PduResponseHandle {
                        index_in_frame: 0,
                        pdu_idx: 2,
                        command_code: 0,
                        // We've used as much of the frame as possible
                        alloc_size: pdu_loop.max_frame_data()
                            - EthernetFrame::<&[u8]>::header_len()
                            - EthercatFrameHeader::header_len()
                    }
                )))
            );

            res.unwrap().unwrap()
        };

        remaining = &remaining[sent..];

        assert_eq!(remaining.len(), 4);

        let (sent, _handle) = {
            let mut frame = pdu_loop.alloc_frame().expect("No frame");

            let res = frame.push_pdu_slice_rest(Command::Nop, remaining);

            // Partial frame as we're at the end of the data we want to send
            let expected_pushed_bytes = 4;

            assert_eq!(
                res,
                Ok(Some((
                    expected_pushed_bytes,
                    PduResponseHandle {
                        index_in_frame: 0,
                        pdu_idx: 3,
                        command_code: 0,
                        // Partial write because we're at the end of our data
                        alloc_size: expected_pushed_bytes + CreatedFrame::PDU_OVERHEAD_BYTES
                    }
                )))
            );

            res.unwrap().unwrap()
        };

        let empty: &[u8] = &[];

        assert_eq!(&remaining[sent..], empty);
    }
}
