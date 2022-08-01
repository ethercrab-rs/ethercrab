use crate::error::PduError;
use crate::pdu::Pdu;
use core::mem::MaybeUninit;
use core::task::Waker;

#[derive(Debug, PartialEq)]
pub(crate) enum FrameState {
    None,
    Created,
    Waiting,
    Done,
}

#[derive(Debug)]
pub(crate) struct Frame<const MAX_PDU_DATA: usize> {
    pub(crate) state: FrameState,
    pub(crate) waker: MaybeUninit<Waker>,
    pub(crate) pdu: MaybeUninit<Pdu<MAX_PDU_DATA>>,
}

impl<const MAX_PDU_DATA: usize> Default for Frame<MAX_PDU_DATA> {
    fn default() -> Self {
        Self {
            state: FrameState::None,
            waker: MaybeUninit::uninit(),
            pdu: MaybeUninit::uninit(),
        }
    }
}

// TODO: Typestates?
impl<const MAX_PDU_DATA: usize> Frame<MAX_PDU_DATA> {
    pub(crate) fn create(&mut self, pdu: Pdu<MAX_PDU_DATA>) {
        self.pdu = MaybeUninit::new(pdu);
        self.state = FrameState::Created;
    }

    pub(crate) fn set_waker(&mut self, waker: &Waker) {
        self.waker = MaybeUninit::new(waker.clone());
    }

    pub(crate) fn wake_done(&mut self, pdu: Pdu<MAX_PDU_DATA>) -> Result<(), PduError> {
        if self.state == FrameState::Waiting {
            let waker = unsafe { self.waker.assume_init_read() };

            pdu.is_response_to(unsafe { self.pdu.assume_init_ref() })?;

            self.pdu = MaybeUninit::new(pdu);
            self.state = FrameState::Done;

            waker.wake();

            Ok(())
        } else {
            Err(PduError::InvalidFrameState)
        }
    }

    /// If there is response data ready, return the data and mark this frame as ready to be reused.
    pub(crate) fn take_ready_data(&mut self) -> Option<Pdu<MAX_PDU_DATA>> {
        match self.state {
            // Response has been received and stored
            FrameState::Done => {
                // Clear frame state ready for reuse
                self.state = FrameState::None;

                // Drop waker so it doesn't get woken again
                unsafe { self.waker.assume_init_drop() };

                Some(unsafe { self.pdu.assume_init_read() })
            }
            // Request hasn't been sent yet, or we're waiting for the response
            _ => None,
        }
    }
}
