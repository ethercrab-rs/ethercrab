use super::{receiving_frame::ReceiveFrameFut, FrameBox, FrameElement, FrameState};

/// A frame in a freshly allocated state.
///
/// This typestate may only be created by
/// [`alloc_frame`](crate::pdu_loop::storage::PduStorageRef::alloc_frame).
#[derive(Debug)]
pub struct CreatedFrame<'sto> {
    pub inner: FrameBox<'sto>,
}

impl<'sto> CreatedFrame<'sto> {
    /// The frame has been initialised, filled with a data payload (if required), and is now ready
    /// to be sent.
    ///
    /// This method returns a future that should be fulfilled when a response to the sent frame is
    /// received.
    pub fn mark_sendable(self) -> ReceiveFrameFut<'sto> {
        unsafe {
            FrameElement::set_state(self.inner.frame, FrameState::Sendable);
        }

        ReceiveFrameFut {
            frame: Some(self.inner),
        }
    }

    pub fn buf_mut(&mut self) -> &mut [u8] {
        unsafe { self.inner.buf_mut() }
    }
}
