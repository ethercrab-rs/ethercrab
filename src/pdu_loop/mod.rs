mod pdu_frame;

use crate::{
    command::Command,
    error::{Error, PduError},
    pdu::Pdu,
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

    fn set_send_waker(&self, waker: &Waker) {
        if self.tx_waker.borrow().is_none() {
            self.tx_waker.borrow_mut().replace(waker.clone());
        }
    }

    pub fn send_frames_blocking<F>(&self, waker: &Waker, mut send: F) -> Result<(), ()>
    where
        F: FnMut(&Pdu<MAX_PDU_DATA>) -> Result<(), ()>,
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

    fn frame(&self, idx: u8) -> Result<&mut pdu_frame::Frame<MAX_PDU_DATA>, Error> {
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
        // TODO: Send zeroes when reading instead of a length
        data_length: u16,
    ) -> Result<Pdu<MAX_PDU_DATA>, Error> {
        let idx = self.idx.fetch_add(1, Ordering::AcqRel) % MAX_FRAMES as u8;

        let frame = self.frame(idx)?;

        let pdu = Pdu::<MAX_PDU_DATA>::new(command, data_length, idx, data)?;

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

    pub fn pdu_rx(&self, ethernet_frame: &[u8]) -> Result<(), Error> {
        let raw_packet = EthernetFrame::new_checked(ethernet_frame)?;

        // Look for EtherCAT packets whilst ignoring broadcast packets sent from self
        if raw_packet.ethertype() != ETHERCAT_ETHERTYPE || raw_packet.src_addr() == MASTER_ADDR {
            return Ok(());
        }

        let (_rest, pdu) = Pdu::<MAX_PDU_DATA>::from_ethernet_payload::<nom::error::Error<&[u8]>>(
            &raw_packet.payload(),
        )
        .map_err(|_| PduError::Parse)?;

        self.frame(pdu.index())?.wake_done(pdu)?;

        Ok(())
    }
}
