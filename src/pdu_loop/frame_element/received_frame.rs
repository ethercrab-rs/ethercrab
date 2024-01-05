use super::{FrameBox, FrameElement, PduFrame};
use crate::{
    fmt,
    pdu_loop::{frame_element::FrameState, PduResponse},
};
use core::{marker::PhantomData, ops::Deref, ptr::NonNull};

/// A frame element where response data has been received from the EtherCAT network.
///
/// A frame may only enter this state when it has been populated with response data from the
/// network.
#[derive(Debug)]
pub struct ReceivedFrame<'sto> {
    pub(in crate::pdu_loop::frame_element) inner: FrameBox<'sto>,
}

impl<'sto> ReceivedFrame<'sto> {
    pub(crate) fn working_counter(&self) -> u16 {
        unsafe { self.inner.frame() }.working_counter
    }

    #[cfg(test)]
    pub fn wkc(self, expected: u16) -> Result<RxFrameDataBuf<'sto>, crate::error::Error> {
        let frame = self.frame();
        let act_wc = frame.working_counter;

        if act_wc == expected {
            Ok(self.into_data_buf())
        } else {
            Err(crate::error::Error::WorkingCounter {
                expected,
                received: act_wc,
            })
        }
    }

    /// Retrieve the frame's internal data and working counter without checking whether the working
    /// counter has a valid value.
    pub fn into_data(self) -> PduResponse<RxFrameDataBuf<'sto>> {
        let wkc = self.working_counter();

        (self.into_data_buf(), wkc)
    }

    fn frame(&self) -> &PduFrame {
        unsafe { self.inner.frame() }
    }

    fn into_data_buf(self) -> RxFrameDataBuf<'sto> {
        let len: usize = self.frame().flags.len().into();

        let sptr = unsafe { FrameElement::buf_ptr(self.inner.frame) };

        RxFrameDataBuf {
            _lt: PhantomData,
            data_start: sptr,
            len,
        }
    }
}

impl<'sto> Drop for ReceivedFrame<'sto> {
    fn drop(&mut self) {
        fmt::trace!("Drop frame element idx {}", self.frame().index);

        unsafe {
            // Invariant: the frame can only be in `RxProcessing` at this point, so if this swap
            // fails there's either a logic bug, or we should panic anyway because the hardware
            // failed.
            fmt::unwrap!(FrameElement::swap_state(
                self.inner.frame,
                FrameState::RxProcessing,
                FrameState::None
            ));
        }
    }
}

pub struct RxFrameDataBuf<'sto> {
    _lt: PhantomData<&'sto ()>,
    data_start: NonNull<u8>,
    len: usize,
}

impl<'sto> core::fmt::Debug for RxFrameDataBuf<'sto> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}

#[cfg(feature = "defmt")]
impl<'sto> defmt::Format for RxFrameDataBuf<'sto> {
    fn format(&self, f: defmt::Formatter) {
        // Format as hexadecimal.
        defmt::write!(f, "{:?}", self);
    }
}

// SAFETY: This is ok because we respect the lifetime of the underlying data by carrying the 'sto
// lifetime.
unsafe impl<'sto> Send for RxFrameDataBuf<'sto> {}

impl<'sto> Deref for RxFrameDataBuf<'sto> {
    type Target = [u8];

    // Temporally shorter borrow: This ref is the lifetime of RxFrameDataBuf, not 'sto. This is the
    // magic.
    fn deref(&self) -> &Self::Target {
        let len = self.len();

        unsafe { core::slice::from_raw_parts(self.data_start.as_ptr(), len) }
    }
}

impl<'sto> RxFrameDataBuf<'sto> {
    pub fn len(&self) -> usize {
        self.len
    }

    pub fn trim_front(&mut self, ct: usize) {
        let ct = ct.min(self.len());

        self.data_start = unsafe { NonNull::new_unchecked(self.data_start.as_ptr().add(ct)) };
    }
}
