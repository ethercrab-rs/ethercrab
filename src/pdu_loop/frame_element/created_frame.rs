use super::{receiving_frame::ReceiveFrameFut, FrameBox, FrameElement, FrameState};
use crate::{
    error::PduError,
    fmt,
    generate::write_packed,
    pdu_loop::{frame_header::EthercatFrameHeader, pdu_flags::PduFlags, pdu_header::PduHeader},
    Command,
};
use core::marker::PhantomData;
use ethercrab_wire::{EtherCrabWireSized, EtherCrabWireWrite};

/// A frame in a freshly allocated state.
///
/// This typestate may only be created by
/// [`alloc_frame`](crate::pdu_loop::storage::PduStorageRef::alloc_frame).
#[derive(Debug)]
pub struct CreatedFrame<'sto> {
    inner: FrameBox<'sto>,
}

impl<'sto> CreatedFrame<'sto> {
    pub(crate) fn new(inner: FrameBox<'sto>) -> CreatedFrame<'sto> {
        Self { inner }
    }

    /// The frame has been initialised, filled with a data payload (if required), and is now ready
    /// to be sent.
    ///
    /// This method returns a future that should be fulfilled when a response to the sent frame is
    /// received.
    pub fn mark_sendable(mut self) -> ReceiveFrameFut<'sto> {
        EthercatFrameHeader::pdu(self.inner.pdu_payload_len() as u16)
            .pack_to_slice_unchecked(unsafe { self.inner.ecat_frame_header_mut() });

        unsafe {
            FrameElement::set_state(self.inner.frame, FrameState::Sendable);
        }

        ReceiveFrameFut {
            frame: Some(self.inner),
        }
    }

    pub fn push_pdu<RX>(
        &mut self,
        command: Command,
        data: impl EtherCrabWireWrite,
        len_override: Option<u16>,
        more_follows: bool,
    ) -> Result<PduResponseHandle<RX>, PduError> {
        let data_length_usize = len_override
            .map(|l| usize::from(l).max(data.packed_len()))
            .unwrap_or(data.packed_len());

        let flags = PduFlags::new(data_length_usize as u16, more_follows);

        // PDU header + data + working counter (space is required for the response value - we never
        // actually write it)
        let alloc_size = data_length_usize + PduHeader::PACKED_LEN + 2;

        let consumed = self.inner.pdu_payload_len();

        // Comprises PDU header, body, working counter
        let buf_range = consumed..(consumed + alloc_size);

        // Establish mapping between this PDU index and the Ethernet frame it's being put in
        let pdu_idx = self.inner.reserve_pdu_marker(self.frame_index())?;

        fmt::trace!(
            "Write PDU {:#04x} into frame index {} ({}, {} bytes at {:?})",
            pdu_idx,
            self.inner.frame_index(),
            command,
            data_length_usize,
            buf_range
        );

        let pdu_buf = unsafe { self.inner.pdu_buf_mut() }
            .get_mut(buf_range.clone())
            .ok_or(PduError::TooLong)?;

        // PDU header
        let pdu_buf = write_packed(command.code(), pdu_buf);
        let pdu_buf = write_packed(pdu_idx, pdu_buf);
        let pdu_buf = write_packed(command, pdu_buf);
        let pdu_buf = write_packed(flags, pdu_buf);
        // IRQ
        let pdu_buf = write_packed(0u16, pdu_buf);

        // Payload
        let _pdu_buf = write_packed(data, pdu_buf);

        // Next two bytes are working counter, but they are always zero on send (and the buffer is
        // zero-initialised) so there's nothing to do.

        self.inner.add_pdu_payload_len(alloc_size);

        Ok(PduResponseHandle {
            _ty: PhantomData,
            buf_start: buf_range.start,
            pdu_idx,
            command_code: command.code(),
        })
    }

    pub fn frame_index(&self) -> u8 {
        self.inner.frame_index()
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

#[derive(Debug)]
pub struct PduResponseHandle<T> {
    _ty: PhantomData<T>,
    /// Offset relative to end of EtherCAT header.
    pub buf_start: usize,
    /// PDU index and command used to validate response match
    pub pdu_idx: u8,
    pub command_code: u8,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pdu_loop::frame_element::{AtomicFrameState, PduMarker};
    use atomic_waker::AtomicWaker;
    use core::{cell::UnsafeCell, mem::MaybeUninit, ptr::NonNull, sync::atomic::AtomicU8};

    #[test]
    fn too_long() {
        let _ = env_logger::builder().is_test(true).try_init();

        const BUF_LEN: usize = 16;

        let pdu_idx = AtomicU8::new(0);

        let mut pdu_states: [PduMarker; 1] = unsafe { MaybeUninit::zeroed().assume_init() };
        pdu_states[0].init();

        let frames = UnsafeCell::new([FrameElement {
            frame_index: 0xab,
            status: AtomicFrameState::new(FrameState::Created),
            waker: AtomicWaker::default(),
            ethernet_frame: [0u8; BUF_LEN],
            pdu_payload_len: 0,
            refcount: AtomicU8::new(0),
        }]);

        // Only one element, and it's the first one, so we don't have to do any pointer arithmetic -
        // just point to the beginning of the array.
        let frame_ptr = frames.get().cast();

        let frame_box = FrameBox::new(
            unsafe { NonNull::new_unchecked(frame_ptr) },
            unsafe { NonNull::new_unchecked(pdu_states.as_mut_ptr()) },
            &pdu_idx,
            BUF_LEN,
        );

        let mut created = CreatedFrame::new(frame_box);

        let handle = created.push_pdu::<u32>(
            Command::fpwr(0x1000, 0x0918).into(),
            [0xffu8; 9],
            None,
            false,
        );

        assert_eq!(handle.unwrap_err(), PduError::TooLong);
    }
}
