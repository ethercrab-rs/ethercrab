use crate::{
    command::Command, error::PduError, pdu::Pdu, timer_factory::TimerFactory, ETHERCAT_ETHERTYPE,
    MASTER_ADDR,
};
use core::{
    cell::RefCell,
    marker::PhantomData,
    sync::atomic::{AtomicU8, Ordering},
    task::{Poll, Waker},
};
use futures::future::{select, Either};
use smoltcp::wire::EthernetFrame;

// TODO: Un-pub
#[derive(Debug, PartialEq)]
pub enum RequestState {
    Created,
    Waiting,
    Done,
}

pub struct PduLoop<const MAX_FRAMES: usize, const MAX_PDU_DATA: usize, TIMEOUT> {
    wakers: RefCell<[Option<Waker>; MAX_FRAMES]>,
    // TODO: Un-pub
    pub frames: RefCell<[Option<(RequestState, Pdu<MAX_PDU_DATA>)>; MAX_FRAMES]>,
    pub send_waker: RefCell<Option<Waker>>,
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
            wakers: RefCell::new([(); MAX_FRAMES].map(|_| None)),
            frames: RefCell::new([(); MAX_FRAMES].map(|_| None)),
            send_waker: RefCell::new(None),
            idx: AtomicU8::new(0),
            _timeout: PhantomData,
        }
    }

    pub async fn pdu(
        &self,
        command: Command,
        data: &[u8],
        data_length: u16,
    ) -> Result<Pdu<MAX_PDU_DATA>, PduError> {
        // braces to ensure we don't hold the refcell across awaits
        let idx = {
            // TODO: Confirm Ordering enum
            let idx = self.idx.fetch_add(1, Ordering::Release) % MAX_FRAMES as u8;

            // We're receiving too fast or the receive buffer isn't long enough
            if self.frames.borrow()[usize::from(idx)].is_some() {
                return Err(PduError::IndexInUse);
            }

            let mut pdu = Pdu::<MAX_PDU_DATA>::new(command, data_length, idx);

            pdu.data = data.try_into().map_err(|_| PduError::TooLong)?;

            // TODO: Races
            self.frames.borrow_mut()[usize::from(idx)] = Some((RequestState::Created, pdu));

            if let Some(waker) = &*self.send_waker.borrow() {
                waker.wake_by_ref()
            }

            usize::from(idx)
        };

        // MSRV: Use core::future::poll_fn when `future_poll_fn ` is stabilised
        let res = futures_lite::future::poll_fn(|ctx| {
            // TODO: Races
            let mut frames = self.frames.borrow_mut();

            let res = frames
                .get_mut(idx)
                .map(|frame| match frame {
                    Some((RequestState::Done, _pdu)) => frame
                        .take()
                        .map(|(_state, pdu)| Poll::Ready(Ok(pdu)))
                        // We shouldn't ever get here because we're already matching against
                        // `Some()`, but the alternative is an `unwrap()` so let's not go there.
                        .unwrap_or(Poll::Pending),
                    _ => Poll::Pending,
                })
                .unwrap_or_else(|| Poll::Ready(Err(PduError::InvalidIndex(idx))));

            self.wakers.borrow_mut()[usize::from(idx)] = Some(ctx.waker().clone());

            res
        });

        // TODO: Configurable timeout
        let timeout = TIMEOUT::timer(core::time::Duration::from_micros(30_000));

        let res = match select(res, timeout).await {
            Either::Left((res, _timeout)) => res,
            Either::Right((_timeout, _res)) => return Err(PduError::Timeout),
        };

        res
    }

    // TODO: Return a result if index is out of bounds, or we don't have a waiting packet
    pub fn parse_response_ethernet_packet(&self, raw_packet: &[u8]) {
        let raw_packet = EthernetFrame::new_unchecked(raw_packet);

        // Look for EtherCAT packets whilst ignoring broadcast packets sent from self
        if raw_packet.ethertype() != ETHERCAT_ETHERTYPE || raw_packet.src_addr() == MASTER_ADDR {
            return ();
        }

        let (_rest, pdu) = Pdu::<MAX_PDU_DATA>::from_ethernet_payload::<nom::error::Error<&[u8]>>(
            &raw_packet.payload(),
        )
        .expect("Packet parse");

        let idx = pdu.index;

        let waker = self.wakers.borrow_mut()[usize::from(idx)].take();

        // Frame is ready; tell everyone about it
        if let Some(waker) = waker {
            // TODO: Borrow races
            if let Some((state, existing_pdu)) = self.frames.borrow_mut()[usize::from(idx)].as_mut()
            {
                pdu.is_response_to(existing_pdu).unwrap();

                *state = RequestState::Done;
                *existing_pdu = pdu
            } else {
                // TODO: Result!
                panic!("No waiting frame for response");
            }

            waker.wake()
        }
    }
}
