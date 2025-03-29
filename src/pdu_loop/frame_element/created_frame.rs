use crate::{
    Command, PduLoop,
    error::PduError,
    fmt,
    generate::write_packed,
    pdu_loop::{
        frame_element::{FrameBox, FrameElement, FrameState, receiving_frame::ReceiveFrameFut},
        frame_header::EthercatFrameHeader,
        pdu_flags::PduFlags,
        pdu_header::PduHeader,
    },
};
use core::{ptr::NonNull, sync::atomic::AtomicU8, time::Duration};
use ethercrab_wire::{
    EtherCrabWireRead, EtherCrabWireSized, EtherCrabWireWrite, EtherCrabWireWriteSized,
};

/// A frame in a freshly allocated state.
///
/// This typestate may only be created by
/// [`alloc_frame`](crate::pdu_loop::storage::PduStorageRef::alloc_frame).
#[derive(Debug)]
pub struct CreatedFrame<'sto> {
    inner: FrameBox<'sto>,
    pdu_count: u8,
    /// Position of the last frame's header in the payload.
    ///
    /// Used for updating the `more_follows` flag when pushing a new PDU.
    last_header_location: Option<usize>,
}

impl<'sto> CreatedFrame<'sto> {
    /// The size of a completely empty PDU.
    ///
    /// Includes header and 2 bytes for working counter.
    pub const PDU_OVERHEAD_BYTES: usize = PduHeader::PACKED_LEN + 2;

    pub(in crate::pdu_loop) fn claim_created(
        frame: NonNull<FrameElement<0>>,
        frame_index: u8,
        pdu_idx: &'sto AtomicU8,
        frame_data_len: usize,
    ) -> Result<Self, PduError> {
        let frame = unsafe { FrameElement::claim_created(frame, frame_index)? };

        let mut inner = FrameBox::new(frame, pdu_idx, frame_data_len);

        inner.init();

        Ok(Self {
            inner,
            pdu_count: 0,
            last_header_location: None,
        })
    }

    pub fn storage_slot_index(&self) -> u8 {
        self.inner.storage_slot_index()
    }

    pub fn is_empty(&self) -> bool {
        self.pdu_count == 0
    }

    /// The frame has been initialised, filled with a data payload (if required), and is now ready
    /// to be sent.
    ///
    /// This method returns a future that should be fulfilled when a response to the sent frame is
    /// received.
    pub fn mark_sendable(
        mut self,
        pdu_loop: &'sto PduLoop<'sto>,
        timeout: Duration,
        retries: usize,
    ) -> ReceiveFrameFut<'sto> {
        EthercatFrameHeader::pdu(self.inner.pdu_payload_len() as u16)
            .pack_to_slice_unchecked(self.inner.ecat_frame_header_mut());

        self.inner.set_state(FrameState::Sendable);

        ReceiveFrameFut {
            frame: Some(self.inner),
            pdu_loop,
            timeout_timer: crate::timer_factory::timer(timeout),
            timeout,
            retries_left: retries,
        }
    }

    /// Push a PDU into this frame, consuming as much space as possible.
    ///
    /// Returns the number of bytes from the given `data` that were written into the frame, or
    /// `None` if the input slice is empty or the frame is full.
    pub(crate) fn push_pdu_slice_rest(
        &mut self,
        command: Command,
        bytes: &[u8],
    ) -> Result<Option<(usize, PduResponseHandle)>, PduError> {
        let consumed = self.inner.pdu_payload_len();

        if bytes.is_empty() {
            return Ok(None);
        }

        // The maximum number of bytes we can insert into this frame
        let max_bytes = self
            .inner
            .pdu_buf()
            .len()
            .saturating_sub(consumed)
            .saturating_sub(Self::PDU_OVERHEAD_BYTES);

        if max_bytes == 0 {
            fmt::trace!("Pushed 0 bytes of {} into PDU", bytes.len());

            return Ok(None);
        }

        let sub_slice_len = max_bytes.min(bytes.packed_len());

        let bytes = &bytes[0..sub_slice_len];

        let flags = PduFlags::new(sub_slice_len as u16, false);

        let alloc_size = sub_slice_len + Self::PDU_OVERHEAD_BYTES;

        let buf_range = consumed..(consumed + alloc_size);

        // Establish mapping between this PDU index and the Ethernet frame it's being put in
        let pdu_idx = self.inner.next_pdu_idx();

        fmt::trace!(
            "Write PDU {:#04x} into rest of frame index {} ({}, {} frame bytes + {} payload bytes at {:?})",
            pdu_idx,
            self.inner.storage_slot_index(),
            command,
            sub_slice_len,
            Self::PDU_OVERHEAD_BYTES,
            buf_range
        );

        let l = self.inner.pdu_buf_mut().len();

        let pdu_buf = self
            .inner
            .pdu_buf_mut()
            .get_mut(buf_range.clone())
            .ok_or_else(|| {
                fmt::error!(
                    "Fill rest of PDU buf range too long: wanted {:?} from {:?}",
                    buf_range,
                    0..l
                );

                PduError::TooLong
            })?;

        let header = PduHeader {
            command_code: command.code(),
            index: pdu_idx,
            command_raw: command.pack(),
            flags,
            irq: 0,
        };

        let pdu_buf = write_packed(header, pdu_buf);

        // Payload
        let _pdu_buf = write_packed(bytes, pdu_buf);

        // Next two bytes are working counter, but they are always zero on send (and the buffer is
        // zero-initialised) so there's nothing to do.

        // Don't need to check length here as we do that with `pdu_buf_mut().get_mut()` above.
        self.inner.add_pdu(alloc_size, pdu_idx);

        let index_in_frame = self.pdu_count;

        self.pdu_count += 1;

        // Frame was added successfully, so now we can update the previous PDU `more_follows` flag to true.
        if let Some(last_header_location) = self.last_header_location.as_mut() {
            // Flags start at 6th bit of header
            let flags_offset = 6usize;

            let last_flags_buf = fmt::unwrap_opt!(
                self.inner
                    .pdu_buf_mut()
                    .get_mut((*last_header_location + flags_offset)..)
            );

            let mut last_flags = fmt::unwrap!(PduFlags::unpack_from_slice(last_flags_buf));

            last_flags.more_follows = true;

            last_flags.pack_to_slice_unchecked(last_flags_buf);

            // Previous header is now the one we just inserted
            *last_header_location = buf_range.start;
        } else {
            self.last_header_location = Some(0);
        }

        Ok(Some((
            sub_slice_len,
            PduResponseHandle {
                index_in_frame,
                pdu_idx,
                command_code: command.code(),
                alloc_size,
            },
        )))
    }

    pub(crate) fn can_push_pdu_payload(&self, packed_len: usize) -> bool {
        let alloc_size = packed_len + Self::PDU_OVERHEAD_BYTES;

        let start_byte = self.inner.pdu_payload_len();

        start_byte + alloc_size <= self.inner.pdu_buf().len()
    }

    /// Push a PDU into this frame.
    ///
    /// # Errors
    ///
    /// Returns [`PduError::TooLong`] if the remaining space in the frame is not enough to hold the
    /// new PDU.
    pub fn push_pdu(
        &mut self,
        command: Command,
        data: impl EtherCrabWireWrite,
        len_override: Option<u16>,
    ) -> Result<PduResponseHandle, PduError> {
        let data_length_usize =
            len_override.map_or(data.packed_len(), |l| usize::from(l).max(data.packed_len()));

        let flags = PduFlags::new(data_length_usize as u16, false);

        // PDU header + data + working counter (space is required for the response value - we never
        // actually write it)
        let alloc_size = data_length_usize + Self::PDU_OVERHEAD_BYTES;

        // The number of payload bytes already consumed in this frame (e.g. from prior PDU
        // insertions). This is the start byte of the current PDU we want to push.
        let start_byte = self.inner.pdu_payload_len();

        // Comprises PDU header, body, working counter
        let buf_range = start_byte..(start_byte + alloc_size);

        // Establish mapping between this PDU index and the Ethernet frame it's being put in
        let pdu_idx = self.inner.next_pdu_idx();

        fmt::trace!(
            "Write PDU {:#04x} into frame index {} ({}, {} bytes at {:?})",
            pdu_idx,
            self.inner.storage_slot_index(),
            command,
            data_length_usize,
            buf_range
        );

        let l = self.inner.pdu_buf_mut().len();

        let pdu_buf = self
            .inner
            .pdu_buf_mut()
            .get_mut(buf_range.clone())
            .ok_or_else(|| {
                fmt::trace!(
                    "Push PDU buf range too long: wanted {:?} from {:?}",
                    buf_range,
                    0..l
                );

                PduError::TooLong
            })?;

        let header = PduHeader {
            command_code: command.code(),
            index: pdu_idx,
            command_raw: command.pack(),
            flags,
            irq: 0,
        };

        let pdu_buf = write_packed(header, pdu_buf);

        // Payload
        let _pdu_buf = write_packed(data, pdu_buf);

        // Next two bytes are working counter, but they are always zero on send (and the buffer is
        // zero-initialised) so there's nothing to do.

        // Don't need to check length here as we do that with `pdu_buf_mut().get_mut()` above.
        self.inner.add_pdu(alloc_size, pdu_idx);

        let index_in_frame = self.pdu_count;

        self.pdu_count += 1;

        // Frame was added successfully, so now we can update the previous PDU `more_follows` flag to true.
        if let Some(last_header_location) = self.last_header_location.as_mut() {
            // Flags start at 6th bit of header
            let flags_offset = 6usize;

            let last_flags_buf = fmt::unwrap_opt!(
                self.inner
                    .pdu_buf_mut()
                    .get_mut((*last_header_location + flags_offset)..)
            );

            let mut last_flags = fmt::unwrap!(PduFlags::unpack_from_slice(last_flags_buf));

            last_flags.more_follows = true;

            last_flags.pack_to_slice_unchecked(last_flags_buf);

            // Previous header is now the one we just inserted
            *last_header_location = buf_range.start;
        } else {
            self.last_header_location = Some(0);
        }

        Ok(PduResponseHandle {
            index_in_frame,
            pdu_idx,
            command_code: command.code(),
            alloc_size,
        })
    }
}

impl<'sto> Drop for CreatedFrame<'sto> {
    fn drop(&mut self) {
        // ONLY free the frame if it's still in created state. If it's been moved into
        // sending/sent/receiving/etc, we must leave it alone.
        let _ = self.inner.swap_state(FrameState::Created, FrameState::None);
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
#[cfg_attr(test, derive(Eq, PartialEq))]
pub struct PduResponseHandle {
    // Might want this in the future
    #[allow(unused)]
    pub index_in_frame: u8,

    /// PDU wire index and command used to validate response match.
    pub pdu_idx: u8,
    pub command_code: u8,

    /// The number of bytes allocated for the PDU header, payload and WKC in the frame.
    pub alloc_size: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        PduStorage, RegisterAddress,
        ethernet::EthernetFrame,
        pdu_loop::frame_element::{AtomicFrameState, FIRST_PDU_EMPTY, FrameElement},
    };
    use atomic_waker::AtomicWaker;
    use core::{
        cell::UnsafeCell,
        ptr::NonNull,
        sync::atomic::{AtomicU8, AtomicU16},
    };

    #[test]
    fn chunked_send() {
        crate::test_logger();

        const MAX_PAYLOAD: usize = 32;

        const BUF_LEN: usize = PduStorage::element_size(MAX_PAYLOAD);

        let pdu_idx = AtomicU8::new(0);

        let frames = UnsafeCell::new([FrameElement {
            storage_slot_index: 0xab,
            status: AtomicFrameState::new(FrameState::None),
            waker: AtomicWaker::default(),
            ethernet_frame: [0u8; BUF_LEN],
            pdu_payload_len: 0,
            first_pdu: AtomicU16::new(FIRST_PDU_EMPTY),
        }]);

        let mut created = CreatedFrame::claim_created(
            unsafe { NonNull::new_unchecked(frames.get().cast()) },
            0xab,
            &pdu_idx,
            BUF_LEN,
        )
        .expect("Claim created");

        let whatever_handle = created.push_pdu(Command::frmw(0x1000, 0x0918).into(), 0u64, None);

        assert!(whatever_handle.is_ok());

        let big_frame = [0xaau8; MAX_PAYLOAD * 2];

        let (rest, _handle) = created
            .push_pdu_slice_rest(Command::fpwr(0x1000, 0x0918).into(), &big_frame)
            .expect("Should not fail")
            .unwrap();

        assert_eq!(rest, 12);
    }

    #[test]
    fn too_long() {
        crate::test_logger();

        const BUF_LEN: usize = 16;

        let pdu_idx = AtomicU8::new(0);

        let frames = UnsafeCell::new([FrameElement {
            storage_slot_index: 0xab,
            status: AtomicFrameState::new(FrameState::None),
            waker: AtomicWaker::default(),
            ethernet_frame: [0u8; BUF_LEN],
            pdu_payload_len: 0,
            first_pdu: AtomicU16::new(FIRST_PDU_EMPTY),
        }]);

        let mut created = CreatedFrame::claim_created(
            unsafe { NonNull::new_unchecked(frames.get().cast()) },
            0xab,
            &pdu_idx,
            BUF_LEN,
        )
        .expect("Claim created");

        let handle = created.push_pdu(Command::fpwr(0x1000, 0x0918).into(), [0xffu8; 9], None);

        assert_eq!(handle.unwrap_err(), PduError::TooLong);
    }

    #[test]
    fn auto_more_follows() {
        crate::test_logger();

        const BUF_LEN: usize = 64;

        let pdu_idx = AtomicU8::new(0);

        let frames = UnsafeCell::new([FrameElement {
            storage_slot_index: 0xab,
            status: AtomicFrameState::new(FrameState::None),
            waker: AtomicWaker::default(),
            ethernet_frame: [0u8; BUF_LEN],
            pdu_payload_len: 0,
            first_pdu: AtomicU16::new(FIRST_PDU_EMPTY),
        }]);

        let mut created = CreatedFrame::claim_created(
            unsafe { NonNull::new_unchecked(frames.get().cast()) },
            0xab,
            &pdu_idx,
            BUF_LEN,
        )
        .expect("Claim created");

        let handle = created.push_pdu(Command::fpwr(0x1000, 0x0918).into(), (), None);
        assert!(handle.is_ok());

        let handle = created.push_pdu(Command::fpwr(0x1001, 0x0918).into(), (), None);
        assert!(handle.is_ok());

        let handle = created.push_pdu(Command::fpwr(0x1002, 0x0918).into(), (), None);
        assert!(handle.is_ok());

        const FLAGS_OFFSET: usize = 6;

        assert_eq!(
            created.inner.pdu_buf()[FLAGS_OFFSET..][..2],
            PduFlags::new(0, true).pack()
        );

        assert_eq!(
            created.inner.pdu_buf()[PduHeader::PACKED_LEN + 2 + FLAGS_OFFSET..][..2],
            PduFlags::new(0, true).pack()
        );

        assert_eq!(
            created.inner.pdu_buf()[(PduHeader::PACKED_LEN + 2 + FLAGS_OFFSET) * 2..][..2],
            PduFlags::new(0, false).pack()
        );
    }

    #[test]
    fn push_rest_too_long() {
        crate::test_logger();

        const BUF_LEN: usize = 32;

        let pdu_idx = AtomicU8::new(0);

        let frames = UnsafeCell::new([FrameElement {
            storage_slot_index: 0xab,
            status: AtomicFrameState::new(FrameState::None),
            waker: AtomicWaker::default(),
            ethernet_frame: [0u8; BUF_LEN],
            pdu_payload_len: 0,
            first_pdu: AtomicU16::new(FIRST_PDU_EMPTY),
        }]);

        let mut created = CreatedFrame::claim_created(
            unsafe { NonNull::new_unchecked(frames.get().cast()) },
            0xab,
            &pdu_idx,
            BUF_LEN,
        )
        .expect("Claim created");

        let data = [0xaau8; 128];

        let res = created.push_pdu_slice_rest(Command::Nop, &data);

        // 32 byte frame contains all the headers with a little bit left over for writing some data
        // into.
        let expected_written = BUF_LEN
            - CreatedFrame::PDU_OVERHEAD_BYTES
            - EthercatFrameHeader::header_len()
            - EthernetFrame::<&[u8]>::header_len();

        // Just double checking
        assert_eq!(expected_written, 4);

        assert_eq!(
            res,
            Ok(Some((
                expected_written,
                PduResponseHandle {
                    index_in_frame: 0,
                    pdu_idx: 0,
                    command_code: 0,
                    // The size of this PDU with a 4 byte payload (plus 12 byte header)
                    alloc_size: 4 + CreatedFrame::PDU_OVERHEAD_BYTES
                }
            )))
        );

        // Can't push anything else
        let res = created.push_pdu_slice_rest(Command::Nop, &data);

        assert_eq!(res, Ok(None));
    }

    #[test]
    fn push_rest_after_dc_sync() {
        crate::test_logger();

        const BUF_LEN: usize = 64;

        let pdu_idx = AtomicU8::new(0);

        let frames = UnsafeCell::new([FrameElement {
            storage_slot_index: 0xab,
            status: AtomicFrameState::new(FrameState::None),
            waker: AtomicWaker::default(),
            ethernet_frame: [0u8; BUF_LEN],
            pdu_payload_len: 0,
            first_pdu: AtomicU16::new(FIRST_PDU_EMPTY),
        }]);

        let mut created = CreatedFrame::claim_created(
            unsafe { NonNull::new_unchecked(frames.get().cast()) },
            0xab,
            &pdu_idx,
            BUF_LEN,
        )
        .expect("Claim created");

        let dc_handle = created
            .push_pdu(
                Command::frmw(0x1000, RegisterAddress::DcSystemTime.into()).into(),
                0u64,
                None,
            )
            .expect("DC handle");

        // 12 byte PDU header plus 8 byte payload
        assert_eq!(dc_handle.alloc_size, 20);

        let data = [0xaau8; 128];

        let remaining = BUF_LEN
            - EthernetFrame::<&[u8]>::header_len()
            - EthercatFrameHeader::header_len()
            - dc_handle.alloc_size;

        // Just double checking
        assert_eq!(remaining, 28);

        let res = created.push_pdu_slice_rest(Command::Nop, &data);

        assert_eq!(
            res,
            Ok(Some((
                remaining - CreatedFrame::PDU_OVERHEAD_BYTES,
                PduResponseHandle {
                    index_in_frame: 1,
                    pdu_idx: 1,
                    command_code: 0,
                    alloc_size: remaining
                }
            )))
        );

        // Can't push anything else
        let res = created.push_pdu_slice_rest(Command::Nop, &data);

        assert_eq!(res, Ok(None));
    }

    #[test]
    fn push_rest_empty() {
        crate::test_logger();

        const BUF_LEN: usize = 64;

        let pdu_idx = AtomicU8::new(0);

        let frames = UnsafeCell::new([FrameElement {
            storage_slot_index: 0xab,
            status: AtomicFrameState::new(FrameState::None),
            waker: AtomicWaker::default(),
            ethernet_frame: [0u8; BUF_LEN],
            pdu_payload_len: 0,
            first_pdu: AtomicU16::new(FIRST_PDU_EMPTY),
        }]);

        let mut created = CreatedFrame::claim_created(
            unsafe { NonNull::new_unchecked(frames.get().cast()) },
            0xab,
            &pdu_idx,
            BUF_LEN,
        )
        .expect("Claim created");

        assert_eq!(
            created.push_pdu_slice_rest(
                Command::frmw(0x1000, RegisterAddress::DcSystemTime.into()).into(),
                &[]
            ),
            Ok(None)
        );
    }
}
