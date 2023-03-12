use super::{received_frame::ReceivedFrame, FrameBox};
use crate::{
    command::Command,
    error::{Error, PduError},
    pdu_loop::{
        frame_element::{FrameElement, FrameState},
        pdu_flags::PduFlags,
    },
};

use core::{
    future::Future,
    ptr::{addr_of_mut, NonNull},
    task::Poll,
};

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
        unsafe { self.set_metadata(flags, irq, working_counter) };

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

        // Frame state must be updated BEFORE the waker is awoken so the future impl returns
        // `Poll::Ready`.
        unsafe {
            FrameElement::set_state(self.inner.frame, FrameState::RxDone);
        }

        waker.wake();

        Ok(())
    }

    unsafe fn set_metadata(&self, flags: PduFlags, irq: u16, working_counter: u16) {
        let frame = NonNull::new_unchecked(addr_of_mut!((*self.inner.frame.as_ptr()).frame));

        *addr_of_mut!((*frame.as_ptr()).flags) = flags;
        *addr_of_mut!((*frame.as_ptr()).irq) = irq;
        *addr_of_mut!((*frame.as_ptr()).working_counter) = working_counter;
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

// SAFETY: This unsafe impl is required due to `FrameBox` containing a `NonNull`, however this impl
// is ok because FrameBox also holds the lifetime `'sto` of the backing store, which is where the
// `NonNull<FrameElement>` comes from.
//
// For example, if the backing storage is is `'static`, we can send things between threads. If it's
// not, the associated lifetime will prevent the framebox from being used in anything that requires
// a 'static bound.
unsafe impl<'sto> Send for ReceiveFrameFut<'sto> {}

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
