mod pdu_frame;

use crate::{
    command::Command, error::PduError, pdu::Pdu, timer_factory::TimerFactory, ETHERCAT_ETHERTYPE,
    MASTER_ADDR,
};
use core::{
    cell::{RefCell, UnsafeCell},
    marker::PhantomData,
    sync::atomic::{AtomicU8, Ordering},
    task::{Poll, Waker},
};
use futures::future::{select, Either};
use smoltcp::wire::EthernetFrame;

pub struct PduLoop<const MAX_FRAMES: usize, const MAX_PDU_DATA: usize, TIMEOUT> {
    frames: [UnsafeCell<pdu_frame::Frame<MAX_PDU_DATA>>; MAX_FRAMES],
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
    pub fn new() -> Self {
        Self {
            frames: [(); MAX_FRAMES].map(|_| UnsafeCell::new(pdu_frame::Frame::default())),
            tx_waker: RefCell::new(None),
            idx: AtomicU8::new(0),
            _timeout: PhantomData,
        }
    }

    // TODO: Un-pub?
    pub fn set_send_waker(&self, waker: &Waker) {
        if self.tx_waker.borrow().is_none() {
            self.tx_waker.borrow_mut().replace(waker.clone());
        }
    }

    pub fn send_frames_blocking<F>(&self, mut send: F) -> Result<(), ()>
    where
        F: FnMut(&Pdu<MAX_PDU_DATA>) -> Result<(), ()>,
    {
        let sendable_frames = self.frames.iter().find_map(|frame| {
            let frame = unsafe { &mut *frame.get() };

            if frame.state == pdu_frame::FrameState::Created {
                Some(frame)
            } else {
                None
            }
        });

        for frame in sendable_frames {
            match send(unsafe { frame.pdu.assume_init_ref() }) {
                Ok(_) => frame.state = pdu_frame::FrameState::Waiting,
                Err(e) => return Err(e),
            }
        }

        Ok(())
    }

    fn frame(&self, idx: u8) -> Result<&mut pdu_frame::Frame<MAX_PDU_DATA>, PduError> {
        let req = self
            .frames
            .get(usize::from(idx))
            .ok_or_else(|| PduError::InvalidIndex(idx))?;

        Ok(unsafe { &mut *req.get() })
    }

    pub async fn pdu_tx(
        &self,
        command: Command,
        data: &[u8],
        data_length: u16,
    ) -> Result<Pdu<MAX_PDU_DATA>, PduError> {
        // Braces to ensure we don't hold the send waker refcell across awaits
        let idx = {
            let idx = self.idx.fetch_add(1, Ordering::Acquire) % MAX_FRAMES as u8;

            let frame = self.frame(idx)?;

            // If a frame slot is in flight and the index wraps back around to it, we're
            // sending/receiving too fast for the given buffer size.
            if frame.state != pdu_frame::FrameState::None {
                return Err(PduError::IndexInUse);
            }

            let mut pdu = Pdu::<MAX_PDU_DATA>::new(command, data_length, idx);
            pdu.data = data.try_into().map_err(|_| PduError::TooLong)?;

            frame.create(pdu);

            // Tell the packet sender there is data ready to send
            match self.tx_waker.try_borrow() {
                Ok(waker) => {
                    if let Some(waker) = &*waker {
                        waker.wake_by_ref()
                    }
                }
                Err(_) => warn!("Send waker is already borrowed"),
            }

            idx
        };

        // MSRV: Use core::future::poll_fn when `future_poll_fn ` is stabilised
        let res = futures_lite::future::poll_fn(|ctx| {
            let frame = match self.frame(idx) {
                Ok(frame) => frame,
                Err(e) => return Poll::Ready(Err(e)),
            };

            frame.set_waker(ctx.waker());

            frame
                .take_ready_data()
                .map(|data| Poll::Ready(Ok(data)))
                .unwrap_or(Poll::Pending)
        });

        // TODO: Configurable timeout
        let timeout = TIMEOUT::timer(core::time::Duration::from_micros(30_000));

        let res = match select(res, timeout).await {
            Either::Left((res, _timeout)) => res,
            Either::Right((_timeout, _res)) => return Err(PduError::Timeout),
        };

        res
    }

    pub fn pdu_rx(&self, raw_packet: &[u8]) -> Result<(), PduError> {
        let raw_packet = EthernetFrame::new_checked(raw_packet)?;

        // Look for EtherCAT packets whilst ignoring broadcast packets sent from self
        if raw_packet.ethertype() != ETHERCAT_ETHERTYPE || raw_packet.src_addr() == MASTER_ADDR {
            return Ok(());
        }

        let (_rest, pdu) = Pdu::<MAX_PDU_DATA>::from_ethernet_payload::<nom::error::Error<&[u8]>>(
            &raw_packet.payload(),
        )
        .map_err(|_| PduError::Parse)?;

        self.frame(pdu.index)?.wake_done(pdu)?;

        Ok(())
    }
}
