use super::{
    received_frame::ReceivedFrame, receiving_frame::ReceiveFrameFut, FrameBox, FrameElement,
    FrameState,
};
use crate::{
    error::{Error, PduError},
    fmt,
    generate::write_packed,
    pdu_loop::{frame_header::EthercatFrameHeader, pdu_flags::PduFlags, pdu_header::PduHeader},
    Command,
};
use core::{marker::PhantomData, ops::Range};
use ethercrab_wire::{EtherCrabWireSized, EtherCrabWireWrite};

/// A frame in a freshly allocated state.
///
/// This typestate may only be created by
/// [`alloc_frame`](crate::pdu_loop::storage::PduStorageRef::alloc_frame).
#[derive(Debug)]
pub struct CreatedFrame<'sto> {
    // NOTE: These are only pub for tests
    inner: FrameBox<'sto>,
    consumed: usize,
}

impl<'sto> CreatedFrame<'sto> {
    pub(crate) fn new(inner: FrameBox<'sto>) -> CreatedFrame<'sto> {
        Self { inner, consumed: 0 }
    }

    /// The frame has been initialised, filled with a data payload (if required), and is now ready
    /// to be sent.
    ///
    /// This method returns a future that should be fulfilled when a response to the sent frame is
    /// received.
    pub fn mark_sendable(self) -> ReceiveFrameFut<'sto> {
        let self_ = self.finish();

        unsafe {
            FrameElement::set_state(self_.inner.frame, FrameState::Sendable);
        }

        ReceiveFrameFut {
            frame: Some(self_.inner),
        }
    }

    /// Write EtherCAT header with length based on how much data has been submitted.
    ///
    /// No more PDUs can be written once the header has been set.
    // NOTE: Pub only for tests
    pub(in crate::pdu_loop) fn finish(mut self) -> Self {
        EthercatFrameHeader::pdu(self.consumed as u16)
            .pack_to_slice_unchecked(unsafe { self.inner.ecat_frame_header_mut() });

        self
    }

    /// Get entire frame buffer. Only really useful for assertions in tests.
    #[cfg(test)]
    pub fn buf(&self) -> &[u8] {
        use smoltcp::wire::EthernetFrame;

        let b = unsafe { self.inner.ethernet_frame() };

        let len =
            EthernetFrame::<&[u8]>::buffer_len(self.consumed) + EthercatFrameHeader::PACKED_LEN;

        &b.into_inner()[0..len]
    }

    #[cfg(test)]
    pub(in crate::pdu_loop) fn inner(self) -> FrameBox<'sto> {
        self.inner
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

        // Comprises PDU header, body, working counter
        let buf_range = self.consumed..(self.consumed + alloc_size);

        // // NOTE: Not using extended length to write payload into. The extended length just reserves
        // // space for incoming data from e.g. an (OOOOIIII) PDI.
        // let payload_range = (self.consumed + PduHeader::PACKED_LEN)
        //     ..(self.consumed + PduHeader::PACKED_LEN + data.packed_len());

        let pdu_idx = self.inner.next_pdu_idx();

        fmt::trace!(
            "Alloc {:?} ({} bytes, {} byte payload) for PDU {:#04x} header and data in frame index {}",
            buf_range,
            alloc_size,
            data_length_usize,
            pdu_idx,
            unsafe { self.inner.frame_index() }
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

        // Establish mapping between this PDU index and the Ethernet frame it's being put in
        let _marker = self.inner.pdu_states[usize::from(pdu_idx)]
            .reserve(self.frame_index(), command, flags)
            // TODO: Better error to explain that the requested PDU is already in flight
            .expect("In use!");

        self.consumed += alloc_size;

        // TODO: Store expected command and stuff too? Maybe better to validate that
        // elsewhere. Not sure where.
        Ok(PduResponseHandle {
            _ty: PhantomData,
            buf_range,
        })
    }

    pub fn frame_index(&self) -> u8 {
        unsafe { self.inner.frame_index() }
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
    buf_range: Range<usize>,
}

impl<T> PduResponseHandle<T> {
    //
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pdu_loop::frame_element::{AtomicFrameState, PduMarker};
    use atomic_waker::AtomicWaker;
    use core::{cell::UnsafeCell, mem::MaybeUninit, ptr::NonNull, sync::atomic::AtomicU8};

    #[test]
    fn push_single() {
        let _ = env_logger::builder().is_test(true).try_init();

        const BUF_LEN: usize = 128;

        let pdu_idx = AtomicU8::new(0);

        let pdu_states: [PduMarker; 1] = unsafe { MaybeUninit::zeroed().assume_init() };
        pdu_states[0].init();

        let frames = UnsafeCell::new([FrameElement {
            frame_index: 0xab,
            status: AtomicFrameState::new(FrameState::Created),
            waker: AtomicWaker::default(),
            ethernet_frame: [0u8; BUF_LEN],
        }]);

        // Only one element, and it's the first one, so we don't have to do any pointer arithmetic -
        // just point to the beginning of the array.
        let frame_ptr = frames.get().cast();

        let frame_box = FrameBox::new(
            unsafe { NonNull::new_unchecked(frame_ptr) },
            &pdu_states,
            &pdu_idx,
            BUF_LEN,
        );

        let mut created = CreatedFrame::new(frame_box);

        let data = 0xAABBCCDDu32;

        let handle = created
            .push_pdu::<u32>(Command::fpwr(0x1000, 0x0918).into(), data, None, false)
            .expect("Push 1");

        dbg!(&created);
    }

    #[test]
    fn too_long() {
        let _ = env_logger::builder().is_test(true).try_init();

        const BUF_LEN: usize = 16;

        let pdu_idx = AtomicU8::new(0);

        let pdu_states: [PduMarker; 1] = unsafe { MaybeUninit::zeroed().assume_init() };
        pdu_states[0].init();

        let frames = UnsafeCell::new([FrameElement {
            frame_index: 0xab,
            status: AtomicFrameState::new(FrameState::Created),
            waker: AtomicWaker::default(),
            ethernet_frame: [0u8; BUF_LEN],
        }]);

        // Only one element, and it's the first one, so we don't have to do any pointer arithmetic -
        // just point to the beginning of the array.
        let frame_ptr = frames.get().cast();

        let frame_box = FrameBox::new(
            unsafe { NonNull::new_unchecked(frame_ptr) },
            &pdu_states,
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
