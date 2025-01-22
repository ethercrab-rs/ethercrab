use crate::{
    error::{Error, PduError},
    fmt,
    pdu_loop::{
        frame_element::{created_frame::PduResponseHandle, FrameBox, FrameState},
        pdu_header::PduHeader,
    },
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
}

impl<'sto> ReceivedFrame<'sto> {
    pub(in crate::pdu_loop) fn new(inner: FrameBox<'sto>) -> ReceivedFrame<'sto> {
        Self { inner }
    }

    pub fn first_pdu(self, handle: PduResponseHandle) -> Result<ReceivedPdu<'sto>, Error> {
        let buf = self.inner.pdu_buf();

        let pdu_header = PduHeader::unpack_from_slice(buf)?;

        let payload_len = usize::from(pdu_header.flags.len());

        // If buffer isn't long enough to hold payload and WKC, this is probably a corrupt PDU or
        // someone is committing epic haxx.
        if buf.len() < payload_len + 2 {
            return Err(Error::Pdu(PduError::TooLong));
        }

        if pdu_header.command_code != handle.command_code {
            return Err(Error::Pdu(PduError::Decode));
        }

        if pdu_header.index != handle.pdu_idx {
            return Err(Error::Pdu(PduError::InvalidIndex(pdu_header.index)));
        }

        let payload_ptr = unsafe {
            NonNull::new_unchecked(
                buf.get(PduHeader::PACKED_LEN..)
                    .ok_or(Error::Internal)?
                    .as_ptr()
                    .cast_mut(),
            )
        };

        let working_counter = u16::unpack_from_slice(
            buf.get((PduHeader::PACKED_LEN + payload_len)..)
                .ok_or(Error::Internal)?,
        )?;

        Ok(ReceivedPdu {
            data_start: payload_ptr,
            len: payload_len,
            working_counter,
            _storage: PhantomData,
        })
    }

    // Might want this in the future
    #[allow(unused)]
    pub fn pdu<'pdu>(&'sto self, handle: PduResponseHandle) -> Result<ReceivedPdu<'pdu>, Error>
    where
        'sto: 'pdu,
    {
        let mut buf = self.inner.pdu_buf();

        // Skip over any preceding PDUs
        for _ in 0..handle.index_in_frame {
            let pdu_header = PduHeader::unpack_from_slice(buf)?;
            let payload_len = usize::from(pdu_header.flags.len());
            let this_pdu_len = PduHeader::PACKED_LEN + payload_len + 2;

            // Start buffer at beginning of next PDU
            buf = buf.get(this_pdu_len..).ok_or(Error::Internal)?;
        }

        // This checks for buffer min length
        let pdu_header = PduHeader::unpack_from_slice(buf)?;

        if pdu_header.command_code != handle.command_code {
            return Err(Error::Pdu(PduError::Decode));
        }

        if pdu_header.index != handle.pdu_idx {
            return Err(Error::Pdu(PduError::InvalidIndex(pdu_header.index)));
        }

        let payload_len = usize::from(pdu_header.flags.len());

        // If buffer isn't long enough to hold payload and WKC, this is probably a corrupt PDU or
        // someone is committing epic haxx.
        if buf.len() < payload_len + 2 {
            return Err(Error::Pdu(PduError::TooLong));
        }

        let payload_ptr = unsafe {
            NonNull::new_unchecked(
                buf.get(PduHeader::PACKED_LEN..)
                    .ok_or(Error::Internal)?
                    .as_ptr()
                    .cast_mut(),
            )
        };

        let working_counter = u16::unpack_from_slice(
            buf.get((PduHeader::PACKED_LEN + payload_len)..)
                .ok_or(Error::Internal)?,
        )?;

        Ok(ReceivedPdu {
            data_start: payload_ptr,
            len: payload_len,
            working_counter,
            _storage: PhantomData,
        })
    }

    pub fn into_pdu_iter(self) -> ReceivedPduIter<'sto> {
        ReceivedPduIter {
            frame: self,
            buf_pos: 0,
        }
    }
}

impl<'sto> Drop for ReceivedFrame<'sto> {
    fn drop(&mut self) {
        // Invariant: the frame can only be in `RxProcessing` at this point, so if this swap fails
        // there's either a logic bug, or we should panic anyway because the hardware failed.
        fmt::unwrap!(self
            .inner
            .swap_state(FrameState::RxProcessing, FrameState::None));

        // Set frame empty sentinel so we don't get false-positive matches when receiving frames
        self.inner.clear_first_pdu();
    }
}

// NOTE: Takes ownership of frame so we can't do double reads with handles
pub struct ReceivedPduIter<'sto> {
    frame: ReceivedFrame<'sto>,
    buf_pos: usize,
}

impl<'sto> Iterator for ReceivedPduIter<'sto> {
    type Item = Result<ReceivedPdu<'sto>, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        let buf = self.frame.inner.pdu_buf().get(self.buf_pos..)?;

        let pdu_header = match PduHeader::unpack_from_slice(buf) {
            Ok(h) => h,
            Err(e) => return Some(Err(e.into())),
        };

        let payload_len = usize::from(pdu_header.flags.len());
        let this_pdu_len = PduHeader::PACKED_LEN + payload_len + 2;

        // If buffer isn't long enough to hold payload and WKC, this is probably a corrupt PDU or
        // someone is committing epic haxx.
        if buf.len() < payload_len + 2 {
            return Some(Err(Error::Pdu(PduError::TooLong)));
        }

        let payload_ptr = unsafe {
            NonNull::new_unchecked(buf.get(PduHeader::PACKED_LEN..)?.as_ptr().cast_mut())
        };

        let working_counter = match buf
            .get((PduHeader::PACKED_LEN + payload_len)..)
            .ok_or(Error::Internal)
            .and_then(|b| u16::unpack_from_slice(b).map_err(Error::from))
        {
            Ok(wkc) => wkc,
            Err(e) => return Some(Err(e)),
        };

        let res = Ok(ReceivedPdu {
            data_start: payload_ptr,
            len: payload_len,
            working_counter,
            _storage: PhantomData,
        });

        // Update buffer pos for next iteration if there are more PDUs to come
        if pdu_header.flags.more_follows {
            self.buf_pos += this_pdu_len;
        }
        // No more frames, so quit the next time round by trying to read way off the end of the
        // buffer.
        else {
            self.buf_pos = usize::MAX
        }

        Some(res)
    }
}

#[derive(Debug)]
pub struct ReceivedPdu<'sto> {
    data_start: NonNull<u8>,
    len: usize,
    pub(crate) working_counter: u16,
    _storage: PhantomData<&'sto ()>,
}

impl<'sto> ReceivedPdu<'sto> {
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

// SAFETY: This is ok because we respect the lifetime of the underlying data by carrying the 'sto
// lifetime.
unsafe impl<'sto> Send for ReceivedPdu<'sto> {}

impl<'sto> Deref for ReceivedPdu<'sto> {
    type Target = [u8];

    // Temporally shorter borrow: This ref is the lifetime of SimpleReceivedPdu, not 'sto. This is
    // the magic.
    fn deref(&self) -> &Self::Target {
        let len = self.len();

        unsafe { core::slice::from_raw_parts(self.data_start.as_ptr(), len) }
    }
}
