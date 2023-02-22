pub mod frame_element;
mod frame_header;
mod pdu;
// pub mod pdu_frame;
mod storage;

use crate::{
    command::{Command, CommandCode},
    error::{Error, PduError, PduValidationError},
    pdu_loop::{
        frame_element::{FrameBox, ReceivedFrame, SendableFrame},
        frame_header::FrameHeader,
        pdu::PduFlags,
        storage::PduStorageRef,
    },
    ETHERCAT_ETHERTYPE, MASTER_ADDR,
};
use core::{
    marker::PhantomData,
    ptr::NonNull,
    sync::atomic::{AtomicU8, Ordering},
    task::Waker,
};
use nom::{
    bytes::complete::take,
    combinator::map_res,
    error::context,
    number::complete::{le_u16, u8},
};
use packed_struct::PackedStructSlice;
use smoltcp::wire::EthernetFrame;
use spin::RwLock;

pub use crate::pdu_loop::{frame_element::FrameElement, storage::PduStorage};

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
#[derive(Debug)]
pub struct PduLoop {
    // frame_data: &'static [UnsafeCell<&'static [u8]>],
    // frames: &'static [UnsafeCell<pdu_frame::Frame>],
    // pub(crate) max_pdu_data: usize,
    storage: PduStorageRef<'static>,

    /// A waker used to wake up the TX task when a new frame is ready to be sent.
    tx_waker: RwLock<Option<Waker>>,
    /// EtherCAT frame index.
    idx: AtomicU8,
}

unsafe impl Sync for PduLoop {}

impl PduLoop {
    /// Create a new PDU loop with the given backing storage.
    pub const fn new(storage: PduStorageRef<'static>) -> Self {
        assert!(storage.num_frames <= u8::MAX as usize);

        Self {
            // frames: storage.frames,
            // frame_data: storage.frame_data,
            // max_pdu_data: storage.max_pdu_data,
            storage,
            tx_waker: RwLock::new(None),
            idx: AtomicU8::new(0),
        }
    }

    pub(crate) fn max_frame_data(&self) -> usize {
        self.storage.frame_data_len
    }

    // TX
    /// Iterate through any PDU TX frames that are ready and send them.
    ///
    /// The blocking `send` function is called for each ready frame. It is given a `SendableFrame`.
    pub fn send_frames_blocking<F>(&self, waker: &Waker, mut send: F) -> Result<(), Error>
    where
        F: FnMut(&SendableFrame<'_>) -> Result<(), ()>,
    {
        for idx in 0..self.storage.num_frames {
            let frame = unsafe { NonNull::new_unchecked(self.storage.frames.as_ptr().add(idx)) };

            let sending = if let Some(frame) = unsafe { FrameElement::claim_sending(frame) } {
                SendableFrame::new(FrameBox {
                    frame,
                    _lifetime: PhantomData,
                })
            } else {
                continue;
            };

            match send(&sending) {
                Ok(_) => {
                    sending.mark_sent();
                }
                Err(_) => {
                    return Err(Error::SendFrame);
                }
            }
        }

        if self.tx_waker.read().is_none() {
            self.tx_waker.write().replace(waker.clone());
        }

        Ok(())
    }

    // TX
    /// Read data back from one or more slave devices.
    pub async fn pdu_tx_readonly(
        &self,
        command: Command,
        data_length: u16,
    ) -> Result<ReceivedFrame<'_>, Error> {
        let idx = self.next_index();

        let frame = self.storage.alloc_frame(command, data_length)?;

        let frame = frame.mark_sendable();

        self.wake_sender();

        let res = frame.await?;

        Ok(res)
    }

    fn next_index(&self) -> u8 {
        self.idx.fetch_add(1, Ordering::AcqRel) % self.storage.num_frames as u8
    }

    /// Tell the packet sender there is data ready to send.
    fn wake_sender(&self) {
        if let Some(waker) = self.tx_waker.read().as_ref() {
            waker.wake_by_ref()
        }
    }

    // TX
    /// Broadcast (BWR) a packet full of zeroes, up to `max_data_length`.
    pub async fn pdu_broadcast_zeros(
        &self,
        register: u16,
        payload_length: u16,
    ) -> Result<ReceivedFrame<'_>, Error> {
        let idx = self.next_index();

        let frame = self.storage.alloc_frame(
            Command::Bwr {
                address: 0,
                register,
            },
            payload_length,
        )?;

        let frame = frame.mark_sendable();

        self.wake_sender();

        let res = frame.await?;

        Ok(res)
    }

    // TX
    /// Send data to and read data back from multiple slaves.
    ///
    /// Unlike [`pdu_tx_readwrite`](crate::pdu_loop::PduLoop::pdu_tx_readwrite), this method allows
    /// overriding the minimum data length of the payload.
    ///
    /// The PDU data length will be the larger of `send_data.len()` and `data_length`. If a larger
    /// response than `send_data` is desired, set the expected response length in `data_length`.
    pub async fn pdu_tx_readwrite_len<'a>(
        &'a self,
        command: Command,
        send_data: &[u8],
        data_length: u16,
    ) -> Result<ReceivedFrame<'_>, Error> {
        let idx = self.next_index();

        let send_data_len = send_data.len();
        let payload_length = u16::try_from(send_data.len())?.max(data_length);

        let mut frame = self.storage.alloc_frame(command, data_length)?;

        let payload = frame
            .buf_mut()
            .get_mut(0..usize::from(payload_length))
            .ok_or(Error::Pdu(PduError::TooLong))?;

        let (data, rest) = payload.split_at_mut(send_data_len);

        data.copy_from_slice(send_data);
        // If we write fewer bytes than the requested payload length (e.g. write SDO with data
        // payload section reserved for reply), make sure the remaining data is zeroed out from any
        // previous request.
        rest.fill(0);

        let frame = frame.mark_sendable();

        self.wake_sender();

        let res = frame.await?;

        Ok(res)
    }

    // TX
    /// Send data to and read data back from multiple slaves.
    pub async fn pdu_tx_readwrite<'a>(
        &'a self,
        command: Command,
        send_data: &[u8],
    ) -> Result<ReceivedFrame<'_>, Error> {
        self.pdu_tx_readwrite_len(command, send_data, send_data.len().try_into()?)
            .await
    }

    // RX
    /// Parse a PDU from a complete Ethernet II frame.
    pub fn pdu_rx(&self, ethernet_frame: &[u8]) -> Result<(), Error> {
        let raw_packet = EthernetFrame::new_checked(ethernet_frame)?;

        // Look for EtherCAT packets whilst ignoring broadcast packets sent from self.
        // As per <https://github.com/OpenEtherCATsociety/SOEM/issues/585#issuecomment-1013688786>,
        // the first slave will set the second bit of the MSB of the MAC address. This means if we
        // send e.g. 10:10:10:10:10:10, we receive 12:10:10:10:10:10 which is useful for this
        // filtering.
        if raw_packet.ethertype() != ETHERCAT_ETHERTYPE || raw_packet.src_addr() == MASTER_ADDR {
            return Ok(());
        }

        let i = raw_packet.payload();

        let (i, header) = context("header", FrameHeader::parse)(i)?;

        // Only take as much as the header says we should
        let (_rest, i) = take(header.payload_len())(i)?;

        let (i, command_code) = map_res(u8, CommandCode::try_from)(i)?;
        let (i, index) = u8(i)?;

        let mut frame = self
            .storage
            .get_receiving(index)
            .ok_or_else(|| PduError::InvalidIndex(usize::from(index)))?;

        if frame.index() != index {
            return Err(Error::Pdu(PduError::Validation(
                PduValidationError::IndexMismatch {
                    sent: frame.index(),
                    received: index,
                },
            )));
        }

        let (i, command) = command_code.parse_address(i)?;

        // Check for weird bugs where a slave might return a different command than the one sent for
        // this PDU index.
        if command.code() != frame.command().code() {
            return Err(Error::Pdu(PduError::Validation(
                PduValidationError::CommandMismatch {
                    sent: command,
                    received: frame.command(),
                },
            )));
        }

        let (i, flags) = map_res(take(2usize), PduFlags::unpack_from_slice)(i)?;
        let (i, irq) = le_u16(i)?;
        let (i, data) = take(flags.length)(i)?;
        let (i, working_counter) = le_u16(i)?;

        log::trace!("Received frame with index {index:#04x}, WKC {working_counter}");

        // `_i` should be empty as we `take()`d an exact amount above.
        debug_assert_eq!(i.len(), 0);

        let frame_data = frame.buf_mut();

        frame_data[0..usize::from(flags.len())].copy_from_slice(data);

        frame.mark_received()?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{storage::PduStorage, *};
    use core::{task::Poll, time::Duration};
    use smoltcp::wire::EthernetAddress;
    use std::thread;

    static STORAGE: PduStorage<16, 128> = PduStorage::<16, 128>::new();
    static PDU_LOOP: PduLoop = PduLoop::new(STORAGE.as_ref());

    // Test the whole TX/RX loop with multiple threads
    #[test]
    fn parallel() {
        // Comment out to make this test work with miri
        // env_logger::try_init().ok();

        let (s, mut r) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();

        thread::Builder::new()
            .name("TX task".to_string())
            .spawn(move || {
                futures_lite::future::block_on(async move {
                    let mut packet_buf = [0u8; 1536];

                    log::info!("Spawn TX task");

                    core::future::poll_fn::<(), _>(move |ctx| {
                        log::info!("Poll fn");

                        PDU_LOOP
                            .send_frames_blocking(ctx.waker(), |frame| {
                                let packet = frame
                                    .write_ethernet_packet(&mut packet_buf)
                                    .expect("Write Ethernet frame");

                                s.send(packet.to_vec()).unwrap();

                                // Simulate packet send delay
                                smol::Timer::after(Duration::from_millis(1));

                                log::info!("Sent packet");

                                Ok(())
                            })
                            .unwrap();

                        Poll::Pending
                    })
                    .await
                })
            })
            .unwrap();

        thread::Builder::new()
            .name("RX task".to_string())
            .spawn(move || {
                futures_lite::future::block_on(async move {
                    log::info!("Spawn RX task");

                    while let Some(ethernet_frame) = r.recv().await {
                        // Munge fake sent frame into a fake received frame
                        let ethernet_frame = {
                            let mut frame = EthernetFrame::new_checked(ethernet_frame).unwrap();
                            frame.set_src_addr(EthernetAddress([
                                0x12, 0x10, 0x10, 0x10, 0x10, 0x10,
                            ]));
                            frame.into_inner()
                        };

                        log::info!("Received packet");

                        PDU_LOOP.pdu_rx(&ethernet_frame).expect("RX");
                    }
                })
            })
            .unwrap();

        // let task_1 = thread::Builder::new()
        //     .name("Task 1".to_string())
        //     .spawn(move || {
        futures_lite::future::block_on(async move {
            for i in 0..64 {
                let data = [0xaa, 0xbb, 0xcc, 0xdd, i];

                log::info!("Send PDU {i}");

                let result = PDU_LOOP
                    .pdu_tx_readwrite(
                        Command::Fpwr {
                            address: 0x1000,
                            register: 0x0980,
                        },
                        &data,
                    )
                    .await
                    .unwrap();

                assert_eq!(result.data(), &data);
            }
        });
        // })
        // .unwrap();

        // futures_lite::future::block_on(async move {
        //     for i in 0..64 {
        //         let data = [0x11, 0x22, 0x33, 0x44, 0x55, i];

        //         log::info!("Send PDU {i}");

        //         let (result, _wkc) = pdu_loop
        //             .pdu_tx_readwrite(
        //                 Command::Fpwr {
        //                     address: 0x1000,
        //                     register: 0x0980,
        //                 },
        //                 &data,
        //                 &Timeouts::default(),
        //             )
        //             .await
        //             .unwrap();

        //         assert_eq!(result, &data);
        //     }
        // });

        // task_1.join().unwrap();
    }
}
