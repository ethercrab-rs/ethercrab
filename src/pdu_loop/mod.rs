mod frame_header;
mod pdu;
pub mod pdu_frame;

use crate::{
    command::{Command, CommandCode},
    error::{Error, PduError, PduValidationError},
    pdu_loop::{frame_header::FrameHeader, pdu::PduFlags, pdu_frame::SendableFrame},
    timer_factory::{timeout, Timeouts, TimerFactory},
    ETHERCAT_ETHERTYPE, MASTER_ADDR,
};
use core::{
    cell::{RefCell, UnsafeCell},
    marker::PhantomData,
    mem::MaybeUninit,
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

pub struct PduLoop<const MAX_FRAMES: usize, const MAX_PDU_DATA: usize, TIMEOUT> {
    frame_data: [UnsafeCell<[u8; MAX_PDU_DATA]>; MAX_FRAMES],
    frames: [UnsafeCell<pdu_frame::Frame>; MAX_FRAMES],
    /// A waker used to wake up the TX task when a new frame is ready to be sent.
    tx_waker: spin::RwLock<Option<Waker>>,
    /// EtherCAT frame index.
    idx: AtomicU8,
    _timeout: PhantomData<TIMEOUT>,
}

unsafe impl<const MAX_FRAMES: usize, const MAX_PDU_DATA: usize, TIMEOUT> Sync
    for PduLoop<MAX_FRAMES, MAX_PDU_DATA, TIMEOUT>
{
}

impl<const MAX_FRAMES: usize, const MAX_PDU_DATA: usize, TIMEOUT>
    PduLoop<MAX_FRAMES, MAX_PDU_DATA, TIMEOUT>
where
    TIMEOUT: TimerFactory,
{
    pub const fn new() -> Self {
        // MSRV: Nightly
        let frames = unsafe { MaybeUninit::zeroed().assume_init() };
        let frame_data = unsafe { MaybeUninit::zeroed().assume_init() };

        assert!(MAX_FRAMES <= u8::MAX as usize);

        Self {
            frames,
            frame_data,
            tx_waker: RwLock::new(None),
            idx: AtomicU8::new(0),
            _timeout: PhantomData,
        }
    }

    // pub fn as_ref(&self) -> PduLoopRef<'_> {
    //     let frame_data = unsafe {
    //         core::slice::from_raw_parts(
    //             self.frame_data.as_ptr() as *const _,
    //             MAX_PDU_DATA * MAX_FRAMES,
    //         )
    //     };

    //     PduLoopRef {
    //         frame_data,
    //         frames: &self.frames,
    //         tx_waker: &self.tx_waker,
    //         idx: &self.idx,
    //         max_pdu_data: MAX_PDU_DATA,
    //         max_frames: MAX_FRAMES,
    //     }
    // }

    // TX
    pub fn send_frames_blocking<F>(&self, waker: &Waker, mut send: F) -> Result<(), Error>
    where
        F: FnMut(&SendableFrame, &[u8]) -> Result<(), ()>,
    {
        for idx in 0..(MAX_FRAMES as u8) {
            let (frame, data) = self.frame(idx)?;

            if let Some(ref mut frame) = frame.sendable() {
                frame.mark_sending();

                send(frame, &data[0..frame.data_len()]).map_err(|_| Error::SendFrame)?;
            }
        }

        if self.tx_waker.read().is_none() {
            self.tx_waker.write().replace(waker.clone());
        }

        Ok(())
    }

    // BOTH
    fn frame(&self, idx: u8) -> Result<(&mut pdu_frame::Frame, &mut [u8]), Error> {
        let idx = usize::from(idx);

        if idx > MAX_FRAMES {
            return Err(Error::Pdu(PduError::InvalidIndex(idx)));
        }

        let frame = unsafe { &mut *self.frames[idx].get() };
        let data = unsafe { &mut *self.frame_data[idx].get() };

        Ok((frame, data))
    }

    // TX
    /// Read data back from one or more slave devices.
    pub async fn pdu_tx_readonly(
        &self,
        command: Command,
        data_length: u16,
        timeouts: &Timeouts,
    ) -> Result<PduResponse<&'_ [u8]>, Error> {
        let idx = self.idx.fetch_add(1, Ordering::AcqRel) % MAX_FRAMES as u8;

        let (frame, frame_data) = self.frame(idx)?;

        // Remove any previous frame's data or other garbage that might be lying around. For
        // performance reasons (maybe - need to bench) this only blanks the portion of the buffer
        // that will be used.
        frame_data[0..usize::from(data_length)].fill(0);

        frame.replace(command, data_length, idx)?;

        self.wake_sender();

        let res = timeout::<TIMEOUT, _, _>(timeouts.pdu, frame).await?;

        Ok((
            &frame_data[0..usize::from(data_length)],
            res.working_counter(),
        ))
    }

    /// Tell the packet sender there is data ready to send.
    fn wake_sender(&self) {
        self.tx_waker
            .read()
            .as_ref()
            .map(|waker| waker.wake_by_ref());
    }

    // TX
    /// Send data to and read data back from multiple slaves.
    ///
    /// Unlike [`pdu_tx_readwrite`], this method allows overriding the minimum data length of the
    /// payload.
    ///
    /// The PDU data length will be the larger of `send_data.len()` and `data_length`. If a larger
    /// response than `send_data` is desired, set the expected response length in `data_length`.
    pub async fn pdu_tx_readwrite_len<'a>(
        &'a self,
        command: Command,
        send_data: &[u8],
        data_length: u16,
        timeouts: &Timeouts,
    ) -> Result<PduResponse<&'a [u8]>, Error> {
        let idx = self.idx.fetch_add(1, Ordering::AcqRel) % MAX_FRAMES as u8;

        let send_data_len = send_data.len();
        let payload_length = u16::try_from(send_data.len())?.max(data_length);

        let (frame, frame_data) = self.frame(idx)?;

        frame.replace(command, payload_length, idx)?;

        let payload = frame_data
            .get_mut(0..usize::from(payload_length))
            .ok_or(Error::Pdu(PduError::TooLong))?;

        let (data, rest) = payload.split_at_mut(send_data_len);

        data.copy_from_slice(send_data);
        // If we write fewer bytes than the requested payload length (e.g. write SDO with data
        // payload section reserved for reply), make sure the remaining data is zeroed out from any
        // previous request.
        rest.fill(0);

        self.wake_sender();

        let res = timeout::<TIMEOUT, _, _>(timeouts.pdu, frame).await?;

        Ok((&payload[0..send_data_len], res.working_counter()))
    }

    // TX
    /// Send data to and read data back from multiple slaves.
    pub async fn pdu_tx_readwrite<'a>(
        &'a self,
        command: Command,
        send_data: &[u8],
        timeouts: &Timeouts,
    ) -> Result<PduResponse<&'a [u8]>, Error> {
        self.pdu_tx_readwrite_len(command, send_data, send_data.len().try_into()?, timeouts)
            .await
    }

    // RX
    pub fn pdu_rx(&self, ethernet_frame: &[u8]) -> Result<(), Error> {
        let raw_packet = EthernetFrame::new_checked(ethernet_frame)?;

        // Look for EtherCAT packets whilst ignoring broadcast packets sent from self
        if raw_packet.ethertype() != ETHERCAT_ETHERTYPE || raw_packet.src_addr() == MASTER_ADDR {
            return Ok(());
        }

        let i = raw_packet.payload();

        let (i, header) = context("header", FrameHeader::parse)(i)?;

        // Only take as much as the header says we should
        let (_rest, i) = take(header.payload_len())(i)?;

        let (i, command_code) = map_res(u8, CommandCode::try_from)(i)?;
        let (i, index) = u8(i)?;

        let (frame, frame_data) = self.frame(index)?;

        if frame.pdu.index != index {
            return Err(Error::Pdu(PduError::Validation(
                PduValidationError::IndexMismatch {
                    sent: frame.pdu.index,
                    received: index,
                },
            )));
        }

        let (i, command) = command_code.parse_address(i)?;

        // Check for weird bugs where a slave might return a different command than the one sent for
        // this PDU index.
        if command.code() != frame.pdu().command().code() {
            return Err(Error::Pdu(PduError::Validation(
                PduValidationError::CommandMismatch {
                    sent: command,
                    received: frame.pdu().command(),
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

        frame_data[0..usize::from(flags.len())].copy_from_slice(data);

        frame.wake_done(flags, irq, working_counter)?;

        Ok(())
    }
}

// TODO: Figure out what to do with this
#[allow(unused)]
pub struct PduLoopRef<'a> {
    frame_data: &'a [UnsafeCell<&'a mut [u8]>],
    frames: &'a [UnsafeCell<pdu_frame::Frame>],
    tx_waker: &'a RefCell<Option<Waker>>,
    idx: &'a AtomicU8,
    max_pdu_data: usize,
    max_frames: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::task::Poll;
    use smoltcp::wire::EthernetAddress;
    use std::{sync::Arc, thread};

    // Test the whole TX/RX loop with multiple threads
    #[test]
    fn parallel() {
        // Comment out to make this test work with miri
        // env_logger::try_init().ok();

        let pdu_loop = Arc::new(PduLoop::<16, 128, smol::Timer>::new());
        let pdu_loop_rx = pdu_loop.clone();
        let pdu_loop_tx = pdu_loop.clone();
        let pdu_loop_1 = pdu_loop.clone();

        let (s, mut r) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();

        thread::spawn(move || {
            smol::block_on(async move {
                let mut packet_buf = [0u8; 1536];

                log::info!("Spawn TX task");

                core::future::poll_fn::<(), _>(move |ctx| {
                    log::info!("Poll fn");

                    pdu_loop_tx
                        .send_frames_blocking(ctx.waker(), |frame, data| {
                            let packet = frame
                                .write_ethernet_packet(&mut packet_buf, data)
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

        thread::spawn(move || {
            smol::block_on(async move {
                log::info!("Spawn RX task");

                while let Some(ethernet_frame) = r.recv().await {
                    // Munge fake sent frame into a fake received frame
                    let ethernet_frame = {
                        let mut frame = EthernetFrame::new_checked(ethernet_frame).unwrap();
                        frame.set_src_addr(EthernetAddress([0x12, 0x10, 0x10, 0x10, 0x10, 0x10]));
                        frame.into_inner()
                    };

                    log::info!("Received packet");

                    pdu_loop_rx.pdu_rx(&ethernet_frame).expect("RX");
                }
            })
        });

        let task_1 = thread::spawn(move || {
            smol::block_on(async move {
                for i in 0..64 {
                    let data = [0xaa, 0xbb, 0xcc, 0xdd, i];

                    log::info!("Send PDU {i}");

                    let (result, _wkc) = pdu_loop_1
                        .pdu_tx_readwrite(
                            Command::Fpwr {
                                address: 0x1000,
                                register: 0x0980,
                            },
                            &data,
                            &Timeouts::default(),
                        )
                        .await
                        .unwrap();

                    assert_eq!(result, &data);
                }
            });
        });

        smol::block_on(async move {
            for i in 0..64 {
                let data = [0x11, 0x22, 0x33, 0x44, 0x55, i];

                log::info!("Send PDU {i}");

                let (result, _wkc) = pdu_loop
                    .pdu_tx_readwrite(
                        Command::Fpwr {
                            address: 0x1000,
                            register: 0x0980,
                        },
                        &data,
                        &Timeouts::default(),
                    )
                    .await
                    .unwrap();

                assert_eq!(result, &data);
            }
        });

        task_1.join().unwrap();
    }
}
