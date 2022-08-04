use crate::error::PduError;
use crate::pdu::Pdu;
use core::future::Future;
use core::mem::MaybeUninit;
use core::pin::Pin;
use core::sync::atomic::{AtomicU8, Ordering};
use core::task::{Context, Poll, Waker};

#[derive(Debug, PartialEq, Eq)]
#[repr(u8)]
enum FrameState {
    None = 0x01,
    Created = 0x02,
    Waiting = 0x04,
    Done = 0x08,
}

impl From<u8> for FrameState {
    fn from(value: u8) -> Self {
        match value {
            0x01 => Self::None,
            0x02 => Self::Created,
            0x04 => Self::Waiting,
            0x08 => Self::Done,
            _ => unreachable!(),
        }
    }
}

#[derive(Debug)]
pub(crate) struct Frame<const MAX_PDU_DATA: usize> {
    state: AtomicU8,
    waker: Option<Waker>,
    pdu: MaybeUninit<Pdu<MAX_PDU_DATA>>,
}

impl<const MAX_PDU_DATA: usize> Default for Frame<MAX_PDU_DATA> {
    fn default() -> Self {
        Self {
            state: AtomicU8::new(FrameState::None as u8),
            waker: None,
            pdu: MaybeUninit::uninit(),
        }
    }
}

impl<const MAX_PDU_DATA: usize> Frame<MAX_PDU_DATA> {
    pub(crate) fn replace(&mut self, pdu: Pdu<MAX_PDU_DATA>) -> Result<(), PduError> {
        trace!("Replace #{}", pdu.index());

        // if self.state.load(Ordering:: SeqCst) != STATE_CREATED {
        //     trace!("Expected {:?}, got {:?}", FrameState::None, self.state);
        //     Err(PduError::InvalidFrameState)?;
        // }

        self.state
            .compare_exchange(
                FrameState::None as u8,
                FrameState::Created as u8,
                Ordering::SeqCst,
                Ordering::SeqCst,
            )
            .map_err(|err| {
                trace!(
                    "replace(): Expected {:?}, got {:?}",
                    FrameState::None,
                    FrameState::from(err)
                );

                PduError::InvalidFrameState
            })?;

        // Ensure we drop any old wakers
        self.waker.take();

        self.pdu = MaybeUninit::new(pdu);

        Ok(())
    }

    pub(crate) fn wake_done(&mut self, pdu: Pdu<MAX_PDU_DATA>) -> Result<(), PduError> {
        trace!("Wake done #{}", pdu.index());

        // if self.state != FrameState::Waiting {
        //     trace!(
        //         "Wake done expected {:?} ({:?}), got {:?} ({:?}). Incoming index {}, current index {}",
        //         FrameState::Waiting,
        //         core::mem::discriminant(&FrameState::Waiting),
        //         self.state,
        //         core::mem::discriminant(&self.state),
        //         pdu.index(),
        //         unsafe { self.pdu.assume_init_ref() }.index()
        //     );

        //     dbg!(&pdu);
        //     dbg!(unsafe { self.pdu.assume_init_ref() });

        //     Err(PduError::InvalidFrameState)?;
        // }

        let mut wait_times = 0;

        // TODO: != Waiting
        while self.state.load(Ordering::Relaxed) == FrameState::Created as u8 {
            wait_times += 1;
        }

        trace!("Waited for state {} times", wait_times);

        // if wait_times > 0 {
        //     panic!("Oh no {wait_times}");
        // }

        pdu.is_response_to(unsafe { self.pdu.assume_init_ref() })?;

        self.state
            .compare_exchange(
                FrameState::Waiting as u8,
                FrameState::Done as u8,
                Ordering::SeqCst,
                Ordering::SeqCst,
            )
            .map_err(|err| {
                trace!(
                    "wake_done(): Expected {:?}, got {:?}",
                    FrameState::Waiting,
                    FrameState::from(err)
                );

                PduError::InvalidFrameState
            })?;

        let idx = pdu.index();
        self.pdu = MaybeUninit::new(pdu);
        // self.state = FrameState::Done;

        let waker = self.waker.take().ok_or_else(|| {
            error!(
                "Attempted to wake frame #{} with no waker, possibly caused by timeout",
                idx
            );

            PduError::InvalidFrameState
        })?;

        trace!("Wake waker #{}: {:?}", idx, waker);

        if wait_times > 0 {
            panic!("Oh no {wait_times}");
        }

        waker.wake();

        Ok(())
    }

    pub(crate) fn sendable<'a>(&'a mut self) -> Option<SendableFrame<'a, MAX_PDU_DATA>> {
        if self.state.load(Ordering::SeqCst) == FrameState::Created as u8 {
            Some(SendableFrame { frame: self })
        } else {
            None
        }
    }
}

/// A frame that is in a sendable state.
pub struct SendableFrame<'a, const MAX_PDU_DATA: usize> {
    frame: &'a mut Frame<MAX_PDU_DATA>,
}

impl<'a, const MAX_PDU_DATA: usize> SendableFrame<'a, MAX_PDU_DATA> {
    #[inline(always)]
    pub(crate) fn mark_sent(&mut self) {
        self.frame
            .state
            .store(FrameState::Waiting as u8, Ordering::SeqCst);
    }

    pub(crate) fn pdu(&self) -> &Pdu<MAX_PDU_DATA> {
        // SAFETY: Because a `SendableFrame` can only be created if the frame is in a created state,
        // we can assume the PDU has been set here.
        unsafe { self.frame.pdu.assume_init_ref() }
    }
}

impl<const MAX_PDU_DATA: usize> Future for Frame<MAX_PDU_DATA> {
    type Output = Result<Pdu<MAX_PDU_DATA>, PduError>;

    fn poll(mut self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<Self::Output> {
        match FrameState::from(self.state.load(Ordering::SeqCst)) {
            FrameState::None => {
                trace!("Frame future polled in None state");
                Poll::Ready(Err(PduError::InvalidFrameState))
            }
            FrameState::Created | FrameState::Waiting => {
                trace!(
                    "Set waker #{}: {:?}",
                    unsafe { self.pdu.assume_init_read() }.index(),
                    ctx.waker()
                );

                // NOTE: Drops previous waker
                self.waker.replace(ctx.waker().clone());

                Poll::Pending
            }
            FrameState::Done => {
                let pdu = unsafe { self.pdu.assume_init_read() };

                // Drop waker so it doesn't get woken again
                self.waker.take();

                // Clear frame state ready for reuse
                self.state.store(FrameState::None as u8, Ordering::SeqCst);

                Poll::Ready(Ok(pdu))
            }
        }
    }
}
