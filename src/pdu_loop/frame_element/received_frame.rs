use super::{created_frame::PduResponseHandle, FrameBox, FrameElement, PduMarker};
use crate::{
    error::{Error, PduError},
    fmt,
    pdu_loop::{frame_element::FrameState, pdu_header::PduHeader, PDU_SLOTS},
    Command,
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
    /// Whether any PDU handles were `take()`n. If this is false, the frame was used in a send-only
    /// capacity, and no [`ReceivedPdu`]s are held. This means `ReceivedFrame` must be responsible
    /// for clearing all the PDU claims normally freed by `ReceivedPdu`'s drop impl.
    unread: Cell<bool>,
}

impl<'sto> ReceivedFrame<'sto> {
    pub fn new(inner: FrameBox<'sto>) -> ReceivedFrame<'sto> {
        Self {
            inner,
            unread: Cell::new(true),
        }
    }

    pub(crate) fn take<'frame, T>(
        &'sto self,
        handle: PduResponseHandle<T>,
    ) -> Result<ReceivedPdu<'frame, T>, Error> {
        // Offset relative to end of EtherCAT header
        let pdu_start_offset = handle.buf_start;

        let this_pdu = &unsafe { self.inner.pdu_buf() }[pdu_start_offset..];

        let pdu_header = PduHeader::unpack_from_slice(this_pdu)?;

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

        let response_command =
            Command::parse_code_data(pdu_header.command_code, pdu_header.command_raw)?;

        if response_command != handle.command {
            fmt::error!(
                "PDU {:#04x} response has incorrect command",
                pdu_header.index
            );

            return Err(Error::Pdu(PduError::Decode));
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

                NonNull::new_unchecked(this_marker)
            },
            working_counter,
            _ty: PhantomData,
            _storage: PhantomData,
            pdu_idx: pdu_header.index,
        })
    }
}

impl<'sto> Drop for ReceivedFrame<'sto> {
    fn drop(&mut self) {
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
    }
}

#[derive(Debug)]
pub struct ReceivedPdu<'sto, T> {
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
