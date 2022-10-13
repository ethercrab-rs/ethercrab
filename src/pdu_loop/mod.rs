mod frame_header;
mod pdu;
pub mod pdu_frame;

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

        assert!(MAX_FRAMES <= u8::MAX as usize);

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

        if self.tx_waker.borrow().is_none() {
            self.tx_waker.borrow_mut().replace(waker.clone());
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
    pub async fn pdu_tx_readonly<'a>(
        &'a self,
        command: Command,
        data_length: u16,
    ) -> Result<PduResponse<&'a [u8]>, Error> {
        let idx = self.idx.fetch_add(1, Ordering::AcqRel) % MAX_FRAMES as u8;

        let (frame, frame_data) = self.frame(idx)?;

        frame.replace(command, data_length, idx)?;

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
    /// Send data to and read data back from multiple slaves.
    ///
    /// The PDU data length will be the large of `send_data.len()` and `data_length`. If a larger
    /// response than `send_data` is desired, set the expected response length in `data_length`.
    pub async fn pdu_tx_readwrite_len<'a>(
        &'a self,
        command: Command,
        send_data: &[u8],
        data_length: u16,
    ) -> Result<PduResponse<&'a [u8]>, Error> {
        let idx = self.idx.fetch_add(1, Ordering::AcqRel) % MAX_FRAMES as u8;

        let send_data_len = usize::from(send_data.len());
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

        Ok((&payload[0..send_data_len], res.working_counter()))
    }

    // TX
    /// Send data to and read data back from multiple slaves.
    pub async fn pdu_tx_readwrite<'a>(
        &'a self,
        command: Command,
        send_data: &[u8],
    ) -> Result<PduResponse<&'a [u8]>, Error> {
        self.pdu_tx_readwrite_len(command, send_data, send_data.len().try_into()?)
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
