mod frame_header;
mod pdu;
mod pdu2;
mod pdu_frame;
mod pdu_frame2;

use crate::{
    command::{Command, CommandCode},
    error::{Error, PduError, PduValidationError},
    pdu_loop::{
        frame_header::FrameHeader,
        pdu::{Pdu, PduFlags},
        pdu_frame::SendableFrame,
    },
    timeout,
    timer_factory::TimerFactory,
    ETHERCAT_ETHERTYPE, MASTER_ADDR,
};
use bbqueue::{BBBuffer, Consumer, GrantW, Producer};
use core::{
    cell::{RefCell, UnsafeCell},
    marker::PhantomData,
    mem::MaybeUninit,
    pin::Pin,
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

use self::pdu_frame2::Frame2;

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

// TODO: Not static lmao
static BUF: BBBuffer<1024> = BBBuffer::new();

pub struct PduLoop<'a, const MAX_FRAMES: usize, const MAX_PDU_DATA: usize, TIMEOUT> {
    // TODO: Can we have a single buffer that gives out variable length slices instead of wasting
    // space with lots of potentially huge PDUs?
    // No, at least not with BBQueue; the received data needs to be written back into the grant, but
    // that means the grant lives too long and blocks the sending of any other data from the
    // BBBuffer.
    frames: [UnsafeCell<pdu_frame::Frame<MAX_PDU_DATA>>; MAX_FRAMES],
    frames2: [UnsafeCell<pdu_frame2::Frame2<'a>>; MAX_FRAMES],
    buf_tx: UnsafeCell<Producer<'static, 1024>>,
    buf_rx: UnsafeCell<Consumer<'static, 1024>>,
    /// A waker used to wake up the TX task when a new frame is ready to be sent.
    tx_waker: RefCell<Option<Waker>>,
    /// EtherCAT frame index.
    idx: AtomicU8,
    _timeout: PhantomData<TIMEOUT>,
}

// If we don't impl Send, does this guarantee we can have a PduLoopRef and not invalidate the
// pointer? BBQueue does this.
unsafe impl<'a, const MAX_FRAMES: usize, const MAX_PDU_DATA: usize, TIMEOUT> Sync
    for PduLoop<'a, MAX_FRAMES, MAX_PDU_DATA, TIMEOUT>
{
}

impl<'a, const MAX_FRAMES: usize, const MAX_PDU_DATA: usize, TIMEOUT>
    PduLoop<'a, MAX_FRAMES, MAX_PDU_DATA, TIMEOUT>
where
    TIMEOUT: TimerFactory,
{
    // // TODO: Make this a const fn so we can store the PDU loop in a static. This will let us give
    // // `Client` and other stuff to other threads, without using scoped threads. I'll need to use
    // // MaybeUninit for `frames`. I also need to move all the methods to `PduLoopRef`, similar to how
    // // BBQueue does it, then initialise the maybeuninit on that call. Maybe we can only get one ref,
    // // but allow `Clone` on it?
    // pub fn new() -> Self {
    //     Self {
    //         frames: [(); MAX_FRAMES].map(|_| UnsafeCell::new(pdu_frame::Frame::default())),
    //         tx_waker: RefCell::new(None),
    //         idx: AtomicU8::new(0),
    //         _timeout: PhantomData,
    //     }
    // }

    // TODO: Re-const
    pub fn new() -> Self {
        let frames = unsafe { MaybeUninit::uninit().assume_init() };
        let frames2 = unsafe { MaybeUninit::uninit().assume_init() };

        let (buf_tx, buf_rx) = BUF.try_split().expect("Buf split");

        Self {
            frames,
            frames2,
            tx_waker: RefCell::new(None),
            idx: AtomicU8::new(0),
            _timeout: PhantomData,
            buf_tx: UnsafeCell::new(buf_tx),
            buf_rx: UnsafeCell::new(buf_rx),
        }
    }

    fn set_send_waker(&self, waker: &Waker) {
        if self.tx_waker.borrow().is_none() {
            self.tx_waker.borrow_mut().replace(waker.clone());
        }
    }

    pub fn send_frames_blocking<F>(&self, waker: &Waker, mut send: F) -> Result<(), ()>
    where
        F: FnMut(&SendableFrame<MAX_PDU_DATA>) -> Result<(), ()>,
    {
        log::trace!("Send frames blocking");

        self.frames.iter().try_for_each(|frame| {
            let frame = unsafe { &mut *frame.get() };

            if let Some(ref mut frame) = frame.sendable() {
                frame.mark_sending();

                send(frame)
            } else {
                Ok(())
            }
        })?;

        self.set_send_waker(waker);

        Ok(())
    }

    fn frame(&self, idx: u8) -> Result<&mut pdu_frame::Frame<MAX_PDU_DATA>, Error> {
        let req = self
            .frames
            .get(usize::from(idx))
            .ok_or(PduError::InvalidIndex(idx))?;

        Ok(unsafe { &mut *req.get() })
    }

    pub async fn pdu_tx(
        &self,
        command: Command,
        data: &[u8],
        data_length: u16,
    ) -> Result<Pdu<MAX_PDU_DATA>, Error> {
        let idx = self.idx.fetch_add(1, Ordering::AcqRel) % MAX_FRAMES as u8;

        let frame = self.frame(idx)?;

        frame.replace(command, data_length, idx, data)?;

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

        timeout::<TIMEOUT, _, _>(timer, frame).await
    }

    fn grant_tx(&self, size: usize) -> GrantW<'static, 1024> {
        let buf_tx = unsafe { &mut *self.buf_tx.get() };

        buf_tx.grant_exact(size).unwrap()
    }

    pub fn stuff(&self) -> &mut Consumer<'static, 1024> {
        let buf_rx = unsafe { &mut *self.buf_rx.get() };

        buf_rx
    }

    fn frame2(&self, idx: u8) -> Result<&mut pdu_frame2::Frame2<'a>, Error> {
        let req = self
            .frames2
            .get(usize::from(idx))
            .ok_or(PduError::InvalidIndex(idx))?;

        Ok(unsafe { &mut *req.get() })
    }

    pub async fn pdu_tx2<'data>(
        &self,
        command: Command,
        data: &'data mut [u8],
        data_length: u16,
    ) -> Result<(&'data [u8], u16), Error> {
        let idx = self.idx.fetch_add(1, Ordering::AcqRel) % MAX_FRAMES as u8;

        // let frame = self.frame(idx)?;

        // frame.replace(command, data_length, idx, data)?;

        let mut frame = Frame2::default();
        // let frame = self.frame2(idx)?;

        frame.replace(command, data_length, idx, data)?;

        let len = frame.ethernet_frame_len();

        let mut grant = self.grant_tx(len);

        frame.to_ethernet_frame(&mut grant)?;

        grant.commit(len);

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

        // let mut slot = self.ref_slot(idx)?;

        // slot.replace(data);

        let working_counter = timeout::<TIMEOUT, _, _>(timer, frame).await?;

        // &data[0..usize::from(data_length)].copy_from_slice(res.data());

        // Ok((data, res.working_counter()))

        Ok((data, working_counter))
    }

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

        frame.wake_done(flags, irq, data, working_counter)?;

        Ok(())
    }
}
