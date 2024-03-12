use super::{FrameBox, FrameElement};
use crate::{
    error::Error,
    fmt,
    pdu_loop::{frame_element::FrameState, pdu_header::PduHeader, PduResponse},
};
use core::{marker::PhantomData, ops::Deref, ptr::NonNull};
use ethercrab_wire::{EtherCrabWireRead, EtherCrabWireSized};

/// A frame element where response data has been received from the EtherCAT network.
///
/// A frame may only enter this state when it has been populated with response data from the
/// network.
#[derive(Debug)]
pub struct ReceivedFrame<'sto> {
    pub(in crate::pdu_loop::frame_element) inner: FrameBox<'sto>,
    offset: usize,
    more_follows: bool,
}

impl<'sto> ReceivedFrame<'sto> {
    pub fn new(inner: FrameBox<'sto>) -> ReceivedFrame<'sto> {
        Self {
            inner,
            offset: 0,
            more_follows: true,
        }
    }

    // pub(crate) fn working_counter(&self) -> u16 {
    //     unsafe { self.inner.frame() }.working_counter
    // }

    // #[cfg(test)]
    // pub fn wkc(self, expected: u16) -> Result<RxFrameDataBuf<'sto>, crate::error::Error> {
    //     let frame = self.frame();
    //     let act_wc = frame.working_counter;

    //     if act_wc == expected {
    //         Ok(self.into_data_buf())
    //     } else {
    //         Err(crate::error::Error::WorkingCounter {
    //             expected,
    //             received: act_wc,
    //         })
    //     }
    // }

    // fn pdus(&self) -> RxFrameDataBuf<'sto> {
    //     let sptr = unsafe { FrameElement::ethercat_payload_ptr(self.inner.frame) };

    //     let len = self.inner.max_len;

    //     RxFrameDataBuf {
    //         _lt: PhantomData,
    //         data_start: sptr,
    //         len,
    //     }
    // }

    pub(crate) fn next_pdu(&mut self) -> Result<Option<PduResponse<RxFrameDataBuf<'sto>>>, Error> {
        // TODO: Validate PDU header against what was sent. Uh how???? lmao

        if !self.more_follows {
            return Ok(None);
        }

        // Make sure buffer is at least large enough to hold a PDU header
        if self.inner.max_len - self.offset < PduHeader::PACKED_LEN {
            fmt::trace!(
                "Not enough space for PDU header: need {}, got {}",
                PduHeader::PACKED_LEN,
                self.inner.max_len - self.offset
            );

            return Err(Error::ReceiveFrame);
        }

        let pdu_ptr = unsafe {
            FrameElement::ethercat_payload_ptr(self.inner.frame)
                .as_ptr()
                .byte_add(self.offset)
                .cast_const()
        };

        let header_buf = unsafe { core::slice::from_raw_parts(pdu_ptr, PduHeader::PACKED_LEN) };

        let header = PduHeader::unpack_from_slice(header_buf)?;

        self.more_follows = header.flags.more_follows;

        let payload_len = usize::from(header.flags.len());

        let remaining = self.inner.max_len - self.offset - PduHeader::PACKED_LEN;

        // Buffer must be large enough to hold PDU payload and working counter
        if remaining < (payload_len + 2) {
            fmt::error!(
                "Not enough space for PDU payload: need {}, got {}",
                payload_len + 2,
                remaining
            );

            return Err(Error::ReceiveFrame);
        }

        let payload_ptr = unsafe {
            NonNull::new_unchecked(
                FrameElement::ethercat_payload_ptr(self.inner.frame)
                    .as_ptr()
                    .byte_add(self.offset + PduHeader::PACKED_LEN),
            )
        };

        let working_counter = {
            let buf = unsafe {
                core::slice::from_raw_parts(
                    FrameElement::ethercat_payload_ptr(self.inner.frame)
                        .as_ptr()
                        .byte_add(self.offset + PduHeader::PACKED_LEN + payload_len)
                        .cast_const(),
                    u16::PACKED_LEN,
                )
            };

            u16::unpack_from_slice(buf)?
        };

        // Add 2 for working counter
        self.offset += PduHeader::PACKED_LEN + payload_len + 2;

        Ok(Some((
            RxFrameDataBuf {
                _lt: PhantomData,
                data_start: payload_ptr,
                len: payload_len,
            },
            working_counter,
        )))
    }

    // fn frame(&self) -> &PduFrame {
    //     unsafe { self.inner.frame() }
    // }

    // fn into_data_buf(self) -> RxFrameDataBuf<'sto> {}
}

impl<'sto> Drop for ReceivedFrame<'sto> {
    fn drop(&mut self) {
        fmt::trace!("Drop frame index {}", unsafe { self.inner.frame_index() });

        self.inner.release_pdu_claims();

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
