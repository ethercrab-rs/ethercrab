use super::{received_frame::ReceivedFrame, FrameBox};
use crate::{
    command::Command,
    error::{Error, PduError},
    pdu_loop::{
        frame_element::{FrameElement, FrameState},
        pdu_flags::PduFlags,
    },
};

use core::{future::Future, task::Poll};

/// A frame has been sent and is now waiting for a response from the network.
///
/// This state may only be entered once the frame has been sent over the network.
#[derive(Debug)]
pub struct ReceivingFrame<'sto> {
    pub inner: FrameBox<'sto>,
}

impl<'sto> ReceivingFrame<'sto> {
    /// Mark the frame as fully received.
    ///
    /// This method may only be called once the frame response (header and data) has been validated
    /// and stored in the frame element.
    pub fn mark_received(
        self,
        flags: PduFlags,
        irq: u16,
        working_counter: u16,
    ) -> Result<(), Error> {
        unsafe { self.inner.set_metadata(flags, irq, working_counter) };

        let frame = unsafe { self.inner.frame() };

        log::trace!("Frame and buf mark_received");

        log::trace!("Mark received, waker is {:?}", frame.waker);

        let waker = unsafe { self.inner.take_waker() }.ok_or_else(|| {
            log::error!(
                "Attempted to wake frame #{} with no waker, possibly caused by timeout",
                frame.index
            );

            PduError::InvalidFrameState
        })?;

        unsafe {
            FrameElement::set_state(self.inner.frame, FrameState::RxDone);
        }

        waker.wake();

        Ok(())
    }

    pub fn buf_mut(&mut self) -> &mut [u8] {
        unsafe { self.inner.buf_mut() }
    }

    pub fn index(&self) -> u8 {
        unsafe { self.inner.frame() }.index
    }

    pub fn command(&self) -> Command {
        unsafe { self.inner.frame() }.command
    }
}

pub struct ReceiveFrameFut<'sto> {
    pub(in crate::pdu_loop::frame_element) frame: Option<FrameBox<'sto>>,
}

impl<'sto> Future for ReceiveFrameFut<'sto> {
    type Output = Result<ReceivedFrame<'sto>, Error>;

    fn poll(
        mut self: core::pin::Pin<&mut Self>,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<Self::Output> {
        let rxin = match self.frame.take() {
            Some(r) => r,
            None => return Poll::Ready(Err(PduError::InvalidFrameState.into())),
        };

        let swappy = unsafe {
            FrameElement::swap_state(rxin.frame, FrameState::RxDone, FrameState::RxProcessing)
        };

        let was = match swappy {
            Ok(_frame_element) => {
                log::trace!("Frame future is ready");
                return Poll::Ready(Ok(ReceivedFrame { inner: rxin }));
            }
            Err(e) => e,
        };

        match was {
            FrameState::Sendable | FrameState::Sending => {
                unsafe { rxin.replace_waker(cx.waker().clone()) };

                self.frame = Some(rxin);

                Poll::Pending
            }
            _ => Poll::Ready(Err(PduError::InvalidFrameState.into())),
        }
    }
}
