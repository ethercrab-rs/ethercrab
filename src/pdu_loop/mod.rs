pub mod frame_element;
mod frame_header;
// mod pdu;
// pub mod pdu_frame;
mod pdu_flags;
mod storage;

use crate::{
    command::{Command, CommandCode},
    error::{Error, PduError, PduValidationError},
    pdu_loop::{
        frame_element::{FrameBox, ReceivedFrame, SendableFrame},
        frame_header::FrameHeader,
        pdu_flags::PduFlags,
        storage::PduStorageRef,
    },
    ETHERCAT_ETHERTYPE, MASTER_ADDR,
};
use core::{marker::PhantomData, ptr::NonNull, task::Waker};
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
            let frame = unsafe { NonNull::new_unchecked(self.storage.frame_at_index(idx)) };

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

        self.tx_waker
            .try_write()
            .expect("update waker contention")
            .replace(waker.clone());

        Ok(())
    }

    // TX
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
        let waker = self
            .tx_waker
            .try_write()
            .expect("wake_sender contention")
            .take();

        log::trace!("Wake sender {:?}", waker);

        if let Some(waker) = waker {
            waker.wake()
        }
    }

    // TX
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

    // TX
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

        // Look for EtherCAT packets whilst ignoring broadcast packets sent from self. As per
        // <https://github.com/OpenEtherCATsociety/SOEM/issues/585#issuecomment-1013688786>, the
        // first slave will set the second bit of the MSB of the MAC address (U/L bit). This means
        // if we send e.g. 10:10:10:10:10:10, we receive 12:10:10:10:10:10 which is useful for this
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
        debug_assert_eq!(i.len(), 0, "trailing data in received frame");

        let frame_data = frame.buf_mut();

        frame_data[0..usize::from(flags.len())].copy_from_slice(data);

        frame.mark_received(flags, irq, working_counter)?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{storage::PduStorage, *};
    use core::task::Poll;
    use smoltcp::wire::EthernetAddress;
    use std::thread;

    static STORAGE: PduStorage<16, 128> = PduStorage::<16, 128>::new();
    static PDU_LOOP: PduLoop = PduLoop::new(STORAGE.as_ref());

    // Test the whole TX/RX loop with multiple threads
    #[test]
    fn parallel() {
        env_logger::try_init().ok();

        let (s, mut r) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();

        let _tx_thread = thread::spawn(move || {
            futures_lite::future::block_on(async move {
                let mut packet_buf = [0u8; 1536];

                log::info!("Spawn TX task");

                core::future::poll_fn::<(), _>(move |ctx| {
                    log::info!("Send poll fn");

                    PDU_LOOP
                        .send_frames_blocking(ctx.waker(), |frame| {
                            let packet = frame
                                .write_ethernet_packet(&mut packet_buf)
                                .expect("Write Ethernet frame");

                            s.send(packet.to_vec()).unwrap();

                            log::info!("Sent packet");

                            Ok(())
                        })
                        .unwrap();

                    Poll::Pending
                })
                .await
            })
        });

        let _rx_thread = thread::spawn(move || {
            futures_lite::future::block_on(async move {
                log::info!("Spawn RX task");

                while let Some(ethernet_frame) = r.recv().await {
                    log::trace!("RX task received packet");

                    // Munge fake sent frame into a fake received frame
                    let ethernet_frame = {
                        let mut frame = EthernetFrame::new_checked(ethernet_frame).unwrap();
                        frame.set_src_addr(EthernetAddress([0x12, 0x10, 0x10, 0x10, 0x10, 0x10]));
                        frame.into_inner()
                    };

                    PDU_LOOP.pdu_rx(&ethernet_frame).expect("RX");
                }
            })
        });

        for i in 0..64 {
            let data = [0xaa, 0xbb, 0xcc, 0xdd, i];

            log::info!("Send PDU {i}");

            let result = futures_lite::future::block_on(PDU_LOOP.pdu_tx_readwrite(
                Command::Fpwr {
                    address: 0x1000,
                    register: 0x0980,
                },
                &data,
            ))
            .unwrap();

            assert_eq!(result.data(), &data);
        }

        log::info!("Sent all PDUs");
    }
}
