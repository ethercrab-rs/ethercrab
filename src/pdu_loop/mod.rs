mod frame_header;
mod pdu;
mod pdu_frame;

use crate::{
    command::{Command, CommandCode},
    error::{Error, PduError, PduValidationError},
    pdu_loop::{frame_header::FrameHeader, pdu::PduFlags, pdu_frame::SendableFrame},
    timeout,
    timer_factory::TimerFactory,
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

// TODO: Move TIMEOUT out of PduLoop. Use it in Client and Eeprom instead
pub struct PduLoop<const MAX_FRAMES: usize, const MAX_PDU_DATA: usize, TIMEOUT> {
    // TODO: Can we have a single buffer that gives out variable length slices instead of wasting
    // space with lots of potentially huge PDUs?
    // No, at least not with BBQueue; the received data needs to be written back into the grant, but
    // that means the grant lives too long and blocks the sending of any other data from the
    // BBBuffer.
    frame_data: [UnsafeCell<[u8; MAX_PDU_DATA]>; MAX_FRAMES],
    frames: [UnsafeCell<pdu_frame::Frame>; MAX_FRAMES],
    /// A waker used to wake up the TX task when a new frame is ready to be sent.
    tx_waker: RefCell<Option<Waker>>,
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

        Self {
            frames,
            frame_data,
            tx_waker: RefCell::new(None),
            idx: AtomicU8::new(0),
            _timeout: PhantomData,
        }
    }

    pub fn as_ref<'a>(&'a self) -> PduLoopRef<'a> {
        let frame_data = unsafe {
            core::slice::from_raw_parts(
                self.frame_data.as_ptr() as *const _,
                MAX_PDU_DATA * MAX_FRAMES,
            )
        };

        PduLoopRef {
            frame_data,
            frames: &self.frames,
            tx_waker: &self.tx_waker,
            idx: &self.idx,
            max_pdu_data: MAX_PDU_DATA,
            max_frames: MAX_FRAMES,
        }
    }

    // TX
    fn set_send_waker(&self, waker: &Waker) {
        if self.tx_waker.borrow().is_none() {
            self.tx_waker.borrow_mut().replace(waker.clone());
        }
    }

    // TX
    pub fn send_frames_blocking<F>(&self, waker: &Waker, mut send: F) -> Result<(), ()>
    where
        F: FnMut(&SendableFrame, &[u8]) -> Result<(), ()>,
    {
        self.frames.iter().try_for_each(|frame| {
            let frame = unsafe { &mut *frame.get() };

            if let Some(ref mut frame) = frame.sendable() {
                let data = self.frame_data(frame.index()).unwrap();

                frame.mark_sending();

                send(frame, &data[0..frame.data_len()])
            } else {
                Ok(())
            }
        })?;

        self.set_send_waker(waker);

        Ok(())
    }

    // BOTH
    fn frame(&self, idx: u8) -> Result<&mut pdu_frame::Frame, Error> {
        let req = self
            .frames
            .get(usize::from(idx))
            .ok_or(PduError::InvalidIndex(idx))?;

        Ok(unsafe { &mut *req.get() })
    }

    // BOTH
    fn frame_data(&self, idx: u8) -> Result<&mut [u8], Error> {
        let req = self
            .frame_data
            .get(usize::from(idx))
            .ok_or(PduError::InvalidIndex(idx))?;

        Ok(unsafe { &mut *req.get() })
    }

    // TX
    pub async fn pdu_tx_readonly<'a>(
        &'a self,
        command: Command,
        // data: &[u8],
        data_length: u16,
    ) -> Result<(&'a [u8], u16), Error> {
        let idx = self.idx.fetch_add(1, Ordering::AcqRel) % MAX_FRAMES as u8;

        let frame = self.frame(idx)?;

        frame.replace(command, data_length, idx)?;

        let frame_data = self.frame_data(idx)?;

        // Tell the packet sender there is data ready to send
        match self.tx_waker.try_borrow() {
            Ok(waker) => {
                if let Some(waker) = &*waker {
                    waker.wake_by_ref()
                }
            }
            Err(_) => warn!("Send waker is already borrowed"),
        }

        // TODO: Configurable timeout
        let timer = core::time::Duration::from_micros(30_000);

        let res = timeout::<TIMEOUT, _, _>(timer, frame).await?;

        Ok((
            &frame_data[0..usize::from(data_length)],
            res.working_counter(),
        ))
    }

    // TX
    pub async fn pdu_tx_readwrite<'a>(
        &'a self,
        command: Command,
        data: &[u8],
        // data_length: u16,
    ) -> Result<(&'a [u8], u16), Error> {
        let idx = self.idx.fetch_add(1, Ordering::AcqRel) % MAX_FRAMES as u8;

        let frame = self.frame(idx)?;

        frame.replace(command, data.len() as u16, idx)?;

        let frame_data = self.frame_data(idx)?;

        frame_data[0..data.len()].copy_from_slice(data);

        // Tell the packet sender there is data ready to send
        match self.tx_waker.try_borrow() {
            Ok(waker) => {
                if let Some(waker) = &*waker {
                    waker.wake_by_ref()
                }
            }
            Err(_) => warn!("Send waker is already borrowed"),
        }

        // TODO: Configurable timeout
        let timer = core::time::Duration::from_micros(30_000);

        let res = timeout::<TIMEOUT, _, _>(timer, frame).await?;

        Ok((
            &frame_data[0..usize::from(data.len())],
            res.working_counter(),
        ))
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

        let frame = self.frame(index)?;

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

        // `_i` should be empty as we `take()`d an exact amount above.
        debug_assert_eq!(i.len(), 0);

        let frame_data = self.frame_data(index)?;
        frame_data[0..usize::from(flags.len())].copy_from_slice(data);

        frame.wake_done(flags, irq, working_counter)?;

        Ok(())
    }
}

// TODO: Figure out what to do with this
pub struct PduLoopRef<'a> {
    frame_data: &'a [UnsafeCell<&'a mut [u8]>],
    frames: &'a [UnsafeCell<pdu_frame::Frame>],
    tx_waker: &'a RefCell<Option<Waker>>,
    idx: &'a AtomicU8,
    max_pdu_data: usize,
    max_frames: usize,
}
