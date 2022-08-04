use crate::error::PduError;
use crate::pdu::Pdu;
use core::future::Future;
use core::mem::MaybeUninit;
use core::pin::Pin;
use core::task::{Context, Poll, Waker};

#[derive(Debug, PartialEq)]
pub(crate) enum FrameState {
    None,
    Created,
    Sending,
    Done,
}

#[derive(Debug)]
pub(crate) struct Frame<const MAX_PDU_DATA: usize> {
    state: FrameState,
    waker: Option<Waker>,
    pdu: MaybeUninit<Pdu<MAX_PDU_DATA>>,
}

impl<const MAX_PDU_DATA: usize> Default for Frame<MAX_PDU_DATA> {
    fn default() -> Self {
        Self {
            state: FrameState::None,
            waker: None,
            pdu: MaybeUninit::uninit(),
        }
    }
}

impl<const MAX_PDU_DATA: usize> Frame<MAX_PDU_DATA> {
    pub(crate) fn replace(&mut self, pdu: Pdu<MAX_PDU_DATA>) -> Result<(), PduError> {
        if self.state != FrameState::None {
            trace!("Expected {:?}, got {:?}", FrameState::None, self.state);
            Err(PduError::InvalidFrameState)?;
        }

        *self = Self {
            state: FrameState::Created,
            waker: None,
            pdu: MaybeUninit::new(pdu),
        };

        Ok(())
    }

    pub(crate) fn wake_done(&mut self, pdu: Pdu<MAX_PDU_DATA>) -> Result<(), PduError> {
        if self.state != FrameState::Sending {
            trace!("Expected {:?}, got {:?}", FrameState::Sending, self.state);
            Err(PduError::InvalidFrameState)?;
        }

        let waker = self.waker.take().ok_or_else(|| {
            error!(
                "Attempted to wake frame #{} with no waker, possibly caused by timeout",
                pdu.index()
            );

            PduError::InvalidFrameState
        })?;

        pdu.is_response_to(unsafe { self.pdu.assume_init_ref() })?;

        self.pdu = MaybeUninit::new(pdu);
        self.state = FrameState::Done;

        waker.wake();

        Ok(())
    }

    pub(crate) fn sendable<'a>(&'a mut self) -> Option<SendableFrame<'a, MAX_PDU_DATA>> {
        if self.state == FrameState::Created {
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
    pub(crate) fn mark_sending(&mut self) {
        self.frame.state = FrameState::Sending;
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
        match self.state {
            FrameState::None => {
                trace!("Frame future polled in None state");
                Poll::Ready(Err(PduError::InvalidFrameState))
            }
            FrameState::Created | FrameState::Sending => {
                // NOTE: Drops previous waker
                self.waker.replace(ctx.waker().clone());

                Poll::Pending
            }
            FrameState::Done => {
                // Clear frame state ready for reuse
                self.state = FrameState::None;

                // Drop waker so it doesn't get woken again
                self.waker.take();

                Poll::Ready(Ok(unsafe { self.pdu.assume_init_read() }))
            }
        }
    }
}
