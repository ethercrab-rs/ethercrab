mod frame_header;
mod pdu;
mod pdu_frame;

use crate::{
    command::Command,
    error::{Error, PduError},
    pdu_loop::pdu::Pdu,
    timeout,
    timer_factory::TimerFactory,
    ETHERCAT_ETHERTYPE, MASTER_ADDR,
};
use core::{
    cell::{RefCell, UnsafeCell},
    marker::PhantomData,
    sync::atomic::{AtomicU8, Ordering},
    task::Waker,
};
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

pub struct PduLoop<'a, const MAX_FRAMES: usize, TIMEOUT> {
    // TODO: Can we have a single buffer that gives out variable length slices instead of wasting
    // space with lots of potentially huge PDUs?
    frames: [UnsafeCell<pdu_frame::Frame<'a>>; MAX_FRAMES],
    /// A waker used to wake up the TX task when a new frame is ready to be sent.
    tx_waker: RefCell<Option<Waker>>,
    /// EtherCAT frame index.
    idx: AtomicU8,
    _timeout: PhantomData<TIMEOUT>,
}

// If we don't impl Send, does this guarantee we can have a PduLoopRef and not invalidate the
// pointer? BBQueue does this.
// TODO: Allow static init of `PduLoop` so it can be given to multiple threads if the user desires.
unsafe impl<'a, const MAX_FRAMES: usize, TIMEOUT> Sync for PduLoop<'a, MAX_FRAMES, TIMEOUT> {}

impl<'a, const MAX_FRAMES: usize, TIMEOUT> PduLoop<'a, MAX_FRAMES, TIMEOUT>
where
    TIMEOUT: TimerFactory,
{
    // TODO: Make this a const fn so we can store the PDU loop in a static. This will let us give
    // `Client` and other stuff to other threads, without using scoped threads. I'll need to use
    // MaybeUninit for `frames`. I also need to move all the methods to `PduLoopRef`, similar to how
    // BBQueue does it, then initialise the maybeuninit on that call. Maybe we can only get one ref,
    // but allow `Clone` on it?
    pub fn new() -> Self {
        Self {
            frames: [(); MAX_FRAMES].map(|_| UnsafeCell::new(pdu_frame::Frame::default())),
            tx_waker: RefCell::new(None),
            idx: AtomicU8::new(0),
            _timeout: PhantomData,
        }
    }

    fn set_send_waker(&self, waker: &Waker) {
        if self.tx_waker.borrow().is_none() {
            self.tx_waker.borrow_mut().replace(waker.clone());
        }
    }

    pub fn send_frames_blocking<F>(&self, waker: &Waker, mut send: F) -> Result<(), ()>
    where
        F: FnMut(&Pdu<'a>) -> Result<(), ()>,
    {
        self.frames.iter().try_for_each(|frame| {
            let frame = unsafe { &mut *frame.get() };

            if let Some(ref mut frame) = frame.sendable() {
                frame.mark_sending();

                send(frame.pdu())
            } else {
                Ok(())
            }
        })?;

        self.set_send_waker(waker);

        Ok(())
    }

    fn frame(&self, idx: u8) -> Result<&mut pdu_frame::Frame<'a>, Error> {
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
    ) -> Result<Pdu<'a>, Error> {
        let idx = self.idx.fetch_add(1, Ordering::AcqRel) % MAX_FRAMES as u8;

        let frame = self.frame(idx)?;

        let pdu = Pdu::new(command, data_length, idx, data)?;

        frame.replace(pdu)?;

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

    pub async fn pdu_tx2<'buf>(
        &self,
        command: Command,
        buffer: &'buf mut [u8],
        read_length: u16,
    ) -> Result<(&'buf [u8], u16), Error> {
        let idx = self.idx.fetch_add(1, Ordering::AcqRel) % MAX_FRAMES as u8;

        let frame = self.frame(idx)?;

        let pdu = Pdu::new(command, read_length, idx, buffer)?;

        frame.replace(pdu)?;

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

        let data = res.data();

        buffer[0..data.len()].copy_from_slice(data);

        Ok((buffer, res.working_counter()))
    }

    pub fn pdu_rx(&self, ethernet_frame: &[u8]) -> Result<(), Error> {
        let raw_packet = EthernetFrame::new_checked(ethernet_frame)?;

        // Look for EtherCAT packets whilst ignoring broadcast packets sent from self
        if raw_packet.ethertype() != ETHERCAT_ETHERTYPE || raw_packet.src_addr() == MASTER_ADDR {
            return Ok(());
        }

        let (_rest, pdu) =
            Pdu::from_ethernet_payload::<nom::error::Error<&[u8]>>(raw_packet.payload())
                .map_err(|_| PduError::Parse)?;

        self.frame(pdu.index())?.wake_done(pdu)?;

        Ok(())
    }
}
