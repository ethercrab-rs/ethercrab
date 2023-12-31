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

    pub fn set_len(&mut self, len: usize) {
        let len = len as u16;

        unsafe { self.inner.frame_mut() }.flags.length = len;
    }

    pub fn index(&self) -> u8 {
        unsafe { self.inner.frame() }.index
    }
}

// SAFETY: This unsafe impl is required due to `FrameBox` containing a `NonNull`, however this impl
// is ok because FrameBox also holds the lifetime `'sto` of the backing store, which is where the
// `NonNull<FrameElement>` comes from.
//
// For example, if the backing storage is is `'static`, we can send things between threads. If it's
// not, the associated lifetime will prevent the framebox from being used in anything that requires
// a 'static bound.
unsafe impl<'sto> Send for CreatedFrame<'sto> {}
