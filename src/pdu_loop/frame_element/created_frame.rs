use super::{receiving_frame::ReceiveFrameFut, FrameBox, FrameElement, FrameState};

#[derive(Debug)]
pub struct CreatedFrame<'sto> {
    pub inner: FrameBox<'sto>,
}

impl<'sto> CreatedFrame<'sto> {
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
