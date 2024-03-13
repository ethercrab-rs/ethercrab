use super::{created_frame::PduResponseHandle, FrameBox, FrameElement, PduMarker};
use crate::{
    error::{Error, PduError},
    fmt,
    pdu_loop::{frame_element::FrameState, pdu_header::PduHeader, PDU_SLOTS},
};
use core::{alloc::Layout, cell::Cell, marker::PhantomData, ops::Deref, ptr::NonNull};
use ethercrab_wire::{EtherCrabWireRead, EtherCrabWireSized};

/// A frame element where response data has been received from the EtherCAT network.
///
/// A frame may only enter this state when it has been populated with response data from the
/// network.
#[derive(Debug)]
pub struct ReceivedFrame<'sto> {
    pub(in crate::pdu_loop::frame_element) inner: FrameBox<'sto>,
    // offset: usize,
    // more_follows: bool,
    // refcount: AtomicU8,
    /// Whether any PDU handles were `take()`n. If this is false, the frame was used in a send-only
    /// capacity, and no [`ReceivedPdu`]s are held. This means `ReceivedFrame` must be responsible
    /// for clearing all the PDU claims normally freed by `ReceivedPdu`'s drop impl.
    unread: Cell<bool>,
}

impl<'sto> ReceivedFrame<'sto> {
    pub fn new(inner: FrameBox<'sto>) -> ReceivedFrame<'sto> {
        Self {
            inner,
            unread: Cell::new(true), // offset: 0,
                                     // more_follows: true,
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

    // #[deprecated(note = "Need to use PDU handles to extract PDU out of raw buffer")]
    // pub(crate) fn next_pdu(&mut self) -> Result<Option<PduResponse<RxFrameDataBuf<'sto>>>, Error> {
    //     // TODO: Validate PDU header against what was sent. Uh how???? lmao

    //     if !self.more_follows {
    //         return Ok(None);
    //     }

    //     // Make sure buffer is at least large enough to hold a PDU header
    //     if self.inner.max_len - self.offset < PduHeader::PACKED_LEN {
    //         fmt::trace!(
    //             "Not enough space for PDU header: need {}, got {}",
    //             PduHeader::PACKED_LEN,
    //             self.inner.max_len - self.offset
    //         );

    //         return Err(Error::ReceiveFrame);
    //     }

    //     let pdu_ptr = unsafe {
    //         FrameElement::ethercat_payload_ptr(self.inner.frame)
    //             .as_ptr()
    //             .byte_add(self.offset)
    //             .cast_const()
    //     };

    //     let header_buf = unsafe { core::slice::from_raw_parts(pdu_ptr, PduHeader::PACKED_LEN) };

    //     let header = PduHeader::unpack_from_slice(header_buf)?;

    //     self.more_follows = header.flags.more_follows;

    //     let payload_len = usize::from(header.flags.len());

    //     let remaining = self.inner.max_len - self.offset - PduHeader::PACKED_LEN;

    //     // Buffer must be large enough to hold PDU payload and working counter
    //     if remaining < (payload_len + 2) {
    //         fmt::error!(
    //             "Not enough space for PDU payload: need {}, got {}",
    //             payload_len + 2,
    //             remaining
    //         );

    //         return Err(Error::ReceiveFrame);
    //     }

    //     let payload_ptr = unsafe {
    //         NonNull::new_unchecked(
    //             FrameElement::ethercat_payload_ptr(self.inner.frame)
    //                 .as_ptr()
    //                 .byte_add(self.offset + PduHeader::PACKED_LEN),
    //         )
    //     };

    //     let working_counter = {
    //         let buf = unsafe {
    //             core::slice::from_raw_parts(
    //                 FrameElement::ethercat_payload_ptr(self.inner.frame)
    //                     .as_ptr()
    //                     .byte_add(self.offset + PduHeader::PACKED_LEN + payload_len)
    //                     .cast_const(),
    //                 u16::PACKED_LEN,
    //             )
    //         };

    //         u16::unpack_from_slice(buf)?
    //     };

    //     // Add 2 for working counter
    //     self.offset += PduHeader::PACKED_LEN + payload_len + 2;

    //     Ok(Some((
    //         RxFrameDataBuf {
    //             _lt: PhantomData,
    //             data_start: payload_ptr,
    //             len: payload_len,
    //         },
    //         working_counter,
    //     )))
    // }

    pub(crate) fn take<'frame, T>(
        &'sto self,
        handle: PduResponseHandle<T>,
    ) -> Result<ReceivedPdu<'frame, T>, Error> {
        // Offset relative to end of EtherCAT header
        let pdu_start_offset = handle.buf_start;

        let this_pdu = &unsafe { self.inner.pdu_buf() }[pdu_start_offset..];

        let pdu_header = PduHeader::unpack_from_slice(this_pdu)?;

        // let payload = rest.split_at(usize::from(pdu_header.flags.len()));

        let payload_len = usize::from(pdu_header.flags.len());

        let payload_ptr = unsafe {
            NonNull::new_unchecked(
                FrameElement::ethercat_payload_ptr(self.inner.frame)
                    .as_ptr()
                    .byte_add(pdu_start_offset + PduHeader::PACKED_LEN),
            )
        };

        let working_counter =
            u16::unpack_from_slice(&this_pdu[(PduHeader::PACKED_LEN + payload_len)..])?;

        // self.refcount.fetch_add(1, Ordering::Acquire);
        FrameElement::<0>::inc_refcount(self.inner.frame);

        if pdu_header.index != handle.pdu_idx {
            fmt::error!(
                "Expected PDU index {:#04x}, got {:#04x}",
                handle.pdu_idx,
                pdu_header.index
            );

            return Err(Error::Pdu(PduError::InvalidIndex(pdu_header.index)));
        }

        self.unread.replace(false);

        Ok(ReceivedPdu {
            data_start: payload_ptr,
            len: payload_len,
            frame: self.inner.frame,
            // frame: &self,
            // frame: self.inner.clone(),
            pdu_marker: unsafe {
                let base_ptr = self.inner.pdu_states.as_ptr();

                let layout = fmt::unwrap!(Layout::array::<PduMarker>(PDU_SLOTS));

                let stride = layout.size() / PDU_SLOTS;

                let this_marker = base_ptr.byte_add(usize::from(pdu_header.index) * stride);

                NonNull::new_unchecked(this_marker as *mut PduMarker)
            },
            working_counter,
            _ty: PhantomData,
            _storage: PhantomData,
            pdu_idx: pdu_header.index,
        })
    }

    // fn frame(&self) -> &PduFrame {
    //     unsafe { self.inner.frame() }
    // }

    // fn into_data_buf(self) -> RxFrameDataBuf<'sto> {}
}

impl<'sto> Drop for ReceivedFrame<'sto> {
    fn drop(&mut self) {
        // for (i, marker) in self.inner.pdu_states.iter().enumerate() {
        //     if marker.frame_index_unchecked() == unsafe { self.inner.frame_index() } {
        //         dbg!(i, marker);
        //     }
        // }

        // No PDU results where `take()`n so we have to free the frame here, instead of relying on
        // `ReceivedPdu::drop`.
        if self.unread.get() {
            fmt::trace!("Frame index {} was untouched, freeing", unsafe {
                self.inner.frame_index()
            });

            self.inner.release_pdu_claims();

            unsafe {
                // Invariant: the frame can only be in `RxProcessing` at this point, so if this
                // swap fails there's either a logic bug, or we should panic anyway because the
                // hardware failed.
                fmt::unwrap!(FrameElement::swap_state(
                    self.inner.frame,
                    FrameState::RxProcessing,
                    FrameState::None
                ));
            }
        }

        // match self.inner.refcount() {
        //     0 => {
        //         fmt::trace!("Drop frame index {} with no PDU handles", unsafe {
        //             self.inner.frame_index()
        //         });

        //     }
        //     n => {
        //         fmt::trace!(
        //             "Frame index {} has {} handles, not dropping",
        //             unsafe { self.inner.frame_index() },
        //             n
        //         );
        //     }
        // }
    }
}

#[derive(Debug)]
pub struct ReceivedPdu<'sto, T> {
    // frame: &'sto ReceivedFrame<'sto>,
    // frame_ref_count: NonNull<AtomicU8>,
    // frame: FrameBox<'sto>,
    pdu_marker: NonNull<PduMarker>,
    frame: NonNull<FrameElement<0>>,
    data_start: NonNull<u8>,
    len: usize,
    pub working_counter: u16,
    _ty: PhantomData<T>,
    _storage: PhantomData<&'sto ()>,
    pdu_idx: u8,
}

impl<'sto, T> ReceivedPdu<'sto, T> {
    pub fn len(&self) -> usize {
        self.len
    }

    pub fn trim_front(&mut self, ct: usize) {
        let ct = ct.min(self.len());

        self.data_start = unsafe { NonNull::new_unchecked(self.data_start.as_ptr().add(ct)) };
    }

    pub fn wkc(self, expected: u16) -> Result<Self, Error> {
        if self.working_counter == expected {
            Ok(self)
        } else {
            Err(Error::WorkingCounter {
                expected,
                received: self.working_counter,
            })
        }
    }

    pub fn maybe_wkc(self, expected: Option<u16>) -> Result<Self, Error> {
        match expected {
            Some(expected) => self.wkc(expected),
            None => Ok(self),
        }
    }
}

// Free up PDU marker for reuse. The frame may still be in use by other PDU handles, so it is not
// dropped here.
//
// The `ReceivedFrame` behind this `ReceivedPdu` can also be dropped, however it does not hold any
// data referenced by `ReceivedPdu` - that is stored in the backing store. The backing store's
// `FrameElement` must remain reserved until all `ReceivedPdu`s are dropped so as to not overwrite
// any referenced data.
impl<'sto, T> Drop for ReceivedPdu<'sto, T> {
    fn drop(&mut self) {
        let frame_idx = u16::from(FrameElement::<0>::frame_index(self.frame));

        unsafe { self.pdu_marker.as_mut() }
            .release_for_frame(frame_idx)
            .expect("Release");

        let old = FrameElement::<0>::dec_refcount(self.frame);

        fmt::trace!(
            "Drop received PDU marker {:#04x}, points to frame index {}, prev refcount {}",
            self.pdu_idx,
            frame_idx,
            old
        );

        // We've just dropped the last handle to the backing store. It can now be released.
        if old == 1 {
            fmt::trace!(
                "All PDU handles dropped, freeing frame element {}",
                FrameElement::<0>::frame_index(self.frame)
            );

            unsafe {
                // Invariant: the frame can only be in `RxProcessing` at this point, so if this swap
                // fails there's either a logic bug, or we should panic anyway because the hardware
                // failed.
                fmt::unwrap!(FrameElement::swap_state(
                    self.frame,
                    FrameState::RxProcessing,
                    FrameState::None
                ));
            }
        }

        // dbg!(self.frame.refcount.fetch_sub(1, Ordering::Release));
    }
}

// SAFETY: This is ok because we respect the lifetime of the underlying data by carrying the 'sto
// lifetime.
unsafe impl<'sto, T> Send for ReceivedPdu<'sto, T> {}

impl<'sto, T> Deref for ReceivedPdu<'sto, T> {
    type Target = [u8];

    // Temporally shorter borrow: This ref is the lifetime of ReceivedPdu, not 'sto. This is the
    // magic.
    fn deref(&self) -> &Self::Target {
        let len = self.len();

        unsafe { core::slice::from_raw_parts(self.data_start.as_ptr(), len) }
    }
}

// pub struct RxFrameDataBuf<'sto> {
//     _lt: PhantomData<&'sto ()>,
//     data_start: NonNull<u8>,
//     len: usize,
// }

// impl<'sto> core::fmt::Debug for RxFrameDataBuf<'sto> {
//     fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
//         f.debug_list().entries(self.iter()).finish()
//     }
// }

// #[cfg(feature = "defmt")]
// impl<'sto> defmt::Format for RxFrameDataBuf<'sto> {
//     fn format(&self, f: defmt::Formatter) {
//         // Format as hexadecimal.
//         defmt::write!(f, "{:?}", self);
//     }
// }

// // SAFETY: This is ok because we respect the lifetime of the underlying data by carrying the 'sto
// // lifetime.
// unsafe impl<'sto> Send for RxFrameDataBuf<'sto> {}

// impl<'sto> Deref for RxFrameDataBuf<'sto> {
//     type Target = [u8];

//     // Temporally shorter borrow: This ref is the lifetime of RxFrameDataBuf, not 'sto. This is the
//     // magic.
//     fn deref(&self) -> &Self::Target {
//         let len = self.len();

//         unsafe { core::slice::from_raw_parts(self.data_start.as_ptr(), len) }
//     }
// }

// impl<'sto> RxFrameDataBuf<'sto> {
//     pub fn len(&self) -> usize {
//         self.len
//     }

//     pub fn trim_front(&mut self, ct: usize) {
//         let ct = ct.min(self.len());

//         self.data_start = unsafe { NonNull::new_unchecked(self.data_start.as_ptr().add(ct)) };
//     }
// }
