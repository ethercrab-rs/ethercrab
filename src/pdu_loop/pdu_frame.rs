use crate::error::{Error, PduError};
use crate::pdu_loop::pdu::Pdu;
use core::future::Future;
use core::pin::Pin;
use core::task::{Context, Poll, Waker};

#[derive(Debug, PartialEq, Default)]
pub(crate) enum FrameState {
    #[default]
    None,
    Created,
    Sending,
    Done,
}

#[derive(Debug, Default)]
pub(crate) struct Frame<'a> {
    state: FrameState,
    waker: Option<Waker>,
    pdu: Pdu<'a>,
}

impl<'a> Frame<'a> {
    pub(crate) fn replace(&mut self, pdu: Pdu<'a>) -> Result<(), PduError> {
        if self.state != FrameState::None {
            trace!("Expected {:?}, got {:?}", FrameState::None, self.state);
            Err(PduError::InvalidFrameState)?;
        }

        *self = Self {
            state: FrameState::Created,
            waker: None,
            pdu,
        };

        Ok(())
    }

    pub(crate) fn wake_done(&mut self, pdu: Pdu<'a>) -> Result<(), PduError> {
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

        pdu.is_response_to(&self.pdu)?;

        self.pdu = pdu;
        self.state = FrameState::Done;

        waker.wake();

        Ok(())
    }

    pub(crate) fn sendable(&mut self) -> Option<SendableFrame<'_>> {
        if self.state == FrameState::Created {
            Some(SendableFrame { frame: self })
        } else {
            None
        }
    }
}

/// A frame that is in a sendable state.
pub struct SendableFrame<'a> {
    frame: &'a mut Frame<'a>,
}

impl<'a> SendableFrame<'a> {
    pub(crate) fn mark_sending(&mut self) {
        self.frame.state = FrameState::Sending;
    }

    pub(crate) fn pdu(&self) -> &Pdu<'a> {
        &self.frame.pdu
    }
}

impl<'a> Future for Frame<'a> {
    type Output = Result<Pdu<'a>, Error>;

    fn poll(mut self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.state {
            FrameState::None => {
                trace!("Frame future polled in None state");
                Poll::Ready(Err(Error::Pdu(PduError::InvalidFrameState)))
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

                Poll::Ready(Ok(core::mem::take(&mut self.pdu)))
            }
        }
    }
}
