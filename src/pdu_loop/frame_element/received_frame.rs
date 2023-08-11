use super::{FrameBox, FrameElement, PduFrame};
use crate::{
    error::Error,
    fmt,
    pdu_loop::{frame_element::FrameState, PduResponse},
};

use core::{ops::Deref, ptr::NonNull};

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

    /// Retrieve the frame's data.
    ///
    /// If the working counter of the received frame does not match the given expected value, this
    /// method will return an [`Error::WorkingCounter`] error.
    pub fn wkc(self, expected: u16, context: &'static str) -> Result<RxFrameDataBuf<'sto>, Error> {
        let frame = self.frame();
        let act_wc = frame.working_counter;

        if act_wc == expected {
            Ok(self.into_data_buf())
        } else {
            Err(Error::WorkingCounter {
                expected,
                received: act_wc,
                context: Some(context),
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
        let eptr = unsafe { NonNull::new_unchecked(sptr.as_ptr().add(len)) };

        RxFrameDataBuf {
            _frame: self,
            data_start: sptr,
            data_end: eptr,
        }
    }
}

impl<'sto> Drop for ReceivedFrame<'sto> {
    fn drop(&mut self) {
        fmt::trace!("Drop frame element idx {}", self.frame().index);

        unsafe { FrameElement::set_state(self.inner.frame, FrameState::None) }
    }
}

pub struct RxFrameDataBuf<'sto> {
    _frame: ReceivedFrame<'sto>,
    data_start: NonNull<u8>,
    data_end: NonNull<u8>,
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
        (self.data_end.as_ptr() as usize) - (self.data_start.as_ptr() as usize)
    }

    pub fn trim_front(&mut self, ct: usize) {
        let sz = self.len();
        if ct > sz {
            self.data_start = self.data_end;
        } else {
            self.data_start = unsafe { NonNull::new_unchecked(self.data_start.as_ptr().add(ct)) };
        }
    }
}
