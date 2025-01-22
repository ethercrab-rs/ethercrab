use super::{frame_header::EthercatFrameHeader, pdu_rx::PduRx, pdu_tx::PduTx};
use crate::ethernet::EthernetFrame;
use crate::{
    error::{Error, PduError},
    fmt,
    pdu_loop::{
        frame_element::{
            created_frame::CreatedFrame, receiving_frame::ReceivingFrame, FrameElement,
        },
        pdu_flags::PduFlags,
    },
    PduLoop,
};
use atomic_waker::AtomicWaker;
use core::{
    alloc::Layout,
    cell::UnsafeCell,
    marker::PhantomData,
    mem::MaybeUninit,
    ptr::NonNull,
    sync::atomic::{AtomicBool, AtomicU8, Ordering},
};
use ethercrab_wire::EtherCrabWireSized;

/// Smallest frame size with a data payload of 0 length
const MIN_DATA: usize = EthernetFrame::<&[u8]>::buffer_len(
    EthercatFrameHeader::header_len()
                    + super::pdu_header::PduHeader::PACKED_LEN
                    // PDU payload
                    + PduFlags::const_default().len() as usize
                    // Working counter
                    + 2,
);

/// Stores PDU frames that are currently being prepared to send, in flight, or being received and
/// processed.
///
/// The number of storage elements `N` must be a power of 2.
pub struct PduStorage<const N: usize, const DATA: usize> {
    frames: UnsafeCell<MaybeUninit<[FrameElement<DATA>; N]>>,
    frame_idx: AtomicU8,
    pdu_idx: AtomicU8,
    is_split: AtomicBool,
    /// A waker used to wake up the TX task when a new frame is ready to be sent.
    pub(in crate::pdu_loop) tx_waker: AtomicWaker,
}

unsafe impl<const N: usize, const DATA: usize> Sync for PduStorage<N, DATA> {}

impl PduStorage<0, 0> {
    /// Calculate the size of a `PduStorage` buffer element to hold the given number of data bytes.
    ///
    /// This computes the additional overhead the Ethernet, EtherCAT frame and EtherCAT PDU headers
    /// require.
    ///
    /// # Examples
    ///
    /// Create a `PduStorage` for a process data image of 128 bytes:
    ///
    /// ```rust
    /// use ethercrab::PduStorage;
    ///
    /// const NUM_FRAMES: usize = 16;
    /// const FRAME_SIZE: usize = PduStorage::element_size(128);
    ///
    /// // 28 byte overhead
    /// assert_eq!(FRAME_SIZE, 156);
    ///
    /// let storage = PduStorage::<NUM_FRAMES, FRAME_SIZE>::new();
    /// ```
    pub const fn element_size(data_len: usize) -> usize {
        MIN_DATA + data_len
    }
}

impl<const N: usize, const DATA: usize> PduStorage<N, DATA> {
    /// Create a new `PduStorage` instance.
    ///
    /// It is recommended to use [`element_size`](PduStorage::element_size) to correctly compute the
    /// overhead required to hold a given PDU payload size.
    ///
    /// # Panics
    ///
    /// This method will panic if
    ///
    /// - `N` is larger than `u8::MAX, or not a power of two, or
    /// - `DATA` is less than 28 as this is the minimum size required to hold an EtherCAT frame with
    ///   zero PDU length.
    pub const fn new() -> Self {
        // MSRV: Make `N` a `u8` when `generic_const_exprs` is stablised
        // If possible, try using `NonZeroU8`.
        // NOTE: Keep max frames in flight at 256 or under. This way, we can guarantee the first PDU
        // in any frame has a unique index.
        assert!(
            N <= u8::MAX as usize,
            "Packet indexes are u8s, so cache array cannot be any bigger than u8::MAX"
        );
        assert!(N > 0, "Storage must contain at least one element");

        assert!(
            DATA >= MIN_DATA,
            "DATA must be at least 28 bytes large to hold all frame headers"
        );

        // Index wrapping limitations require a power of 2 number of storage elements.
        if N > 1 {
            assert!(
                N.count_ones() == 1,
                "The number of storage elements must be a power of 2"
            );
        }

        let frames = UnsafeCell::new(MaybeUninit::zeroed());

        Self {
            frames,
            frame_idx: AtomicU8::new(0),
            pdu_idx: AtomicU8::new(0),
            is_split: AtomicBool::new(false),
            tx_waker: AtomicWaker::new(),
        }
    }

    /// Create a PDU loop backed by this storage.
    ///
    /// Returns a TX and RX driver, and a handle to the PDU loop. This method will return an error
    /// if called more than once.
    ///
    /// # Errors
    ///
    /// To maintain ownership and lifetime invariants, `try_split` will return an error if called
    /// more than once on any given `PduStorage`.
    #[allow(clippy::result_unit_err)]
    pub fn try_split(&self) -> Result<(PduTx<'_>, PduRx<'_>, PduLoop<'_>), ()> {
        self.is_split
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Relaxed)
            // TODO: Make try_split const when ? is allowed in const methods, tracking issue
            // <https://github.com/rust-lang/rust/issues/74935>
            .map_err(|_| ())?;

        let storage = self.as_ref();

        Ok((
            PduTx::new(storage.clone()),
            PduRx::new(storage.clone()),
            PduLoop::new(storage),
        ))
    }

    fn as_ref(&self) -> PduStorageRef {
        PduStorageRef {
            frames: unsafe { NonNull::new_unchecked(self.frames.get().cast()) },
            frame_element_stride: Layout::array::<FrameElement<DATA>>(N).unwrap().size() / N,
            num_frames: N,
            frame_data_len: DATA,
            frame_idx: &self.frame_idx,
            pdu_idx: &self.pdu_idx,
            tx_waker: &self.tx_waker,
            _lifetime: PhantomData,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct PduStorageRef<'sto> {
    frames: NonNull<FrameElement<0>>,
    /// Stride in bytes used to calculate frame element index pointer offsets.
    frame_element_stride: usize,
    pub num_frames: usize,
    pub frame_data_len: usize,
    frame_idx: &'sto AtomicU8,
    pub pdu_idx: &'sto AtomicU8,
    pub tx_waker: &'sto AtomicWaker,
    _lifetime: PhantomData<&'sto ()>,
}

impl<'sto> PduStorageRef<'sto> {
    /// Allocate a PDU frame with the given command and data length.
    pub(in crate::pdu_loop) fn alloc_frame(&self) -> Result<CreatedFrame<'sto>, Error> {
        // Find next frame that is not currently in use.
        //
        // Escape hatch: we'll only loop through the frame storage array twice to put an upper
        // bound on the number of times this loop can execute. It could be allowed to execute
        // indefinitely and rely on PDU future timeouts to cancel, but that seems brittle hence
        // this safety check.
        //
        // This can be mitigated by using a `RetryBehaviour` of `Count` or `Forever`.
        for _ in 0..(self.num_frames * 2) {
            let frame_idx = self.frame_idx.fetch_add(1, Ordering::Relaxed) % self.num_frames as u8;

            fmt::trace!("Try to allocate frame {}", frame_idx);

            // Claim frame so it has a unique owner until its response data is dropped. It must be
            // claimed before initialisation to avoid race conditions with other threads potentially
            // claiming the same frame. The race conditions are mitigated by an atomic state
            // variable in the frame, and the atomic index counter above.
            let frame = self.frame_at_index(usize::from(frame_idx));

            let frame =
                CreatedFrame::claim_created(frame, frame_idx, self.pdu_idx, self.frame_data_len);

            if let Ok(f) = frame {
                return Ok(f);
            }
        }

        // We've searched twice and found no free slots. This means the application should
        // either slow down its packet sends, or increase `N` in `PduStorage` as there
        // aren't enough slots to hold all in-flight packets.
        fmt::error!("No available frames in {} slots", self.num_frames);

        Err(PduError::SwapState.into())
    }

    /// Updates state from SENDING -> RX_BUSY
    pub(in crate::pdu_loop) fn claim_receiving(
        &self,
        frame_idx: u8,
    ) -> Option<ReceivingFrame<'sto>> {
        let frame_idx = usize::from(frame_idx);

        if frame_idx >= self.num_frames {
            return None;
        }

        fmt::trace!("--> Claim receiving frame index {}", frame_idx);

        ReceivingFrame::claim_receiving(
            self.frame_at_index(frame_idx),
            self.pdu_idx,
            self.frame_data_len,
        )
    }

    pub(in crate::pdu_loop) fn frame_index_by_first_pdu_index(
        &self,
        search_pdu_idx: u8,
    ) -> Option<u8> {
        for frame_index in 0..self.num_frames {
            // SAFETY: Frames pointer will always be non-null as it was created by Rust code.
            let frame = unsafe {
                NonNull::new_unchecked(
                    self.frames
                        .as_ptr()
                        .byte_add(frame_index * self.frame_element_stride),
                )
            };

            if unsafe { FrameElement::<0>::first_pdu_is(frame, search_pdu_idx) } {
                return Some(frame_index as u8);
            }
        }

        None
    }

    /// Retrieve a frame at the given index.
    ///
    /// If the given index is greater than the value in `PduStorage::N`, this will return garbage
    /// data off the end of the frame element buffer.
    pub(crate) fn frame_at_index(&self, idx: usize) -> NonNull<FrameElement<0>> {
        assert!(idx < self.num_frames);

        // SAFETY: `self.frames` was created by Rust, so will always be valid. The index is checked
        // that it doesn't extend past the end of the storage array above, so we should never return
        // garbage data as long as `self.frame_element_stride` is computed correctly.
        unsafe {
            NonNull::new_unchecked(
                self.frames
                    .as_ptr()
                    .byte_add(idx * self.frame_element_stride),
            )
        }
    }
}

unsafe impl<'sto> Send for PduStorageRef<'sto> {}
unsafe impl<'sto> Sync for PduStorageRef<'sto> {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{pdu_loop::pdu_header::PduHeader, Command};
    use core::time::Duration;

    #[test]
    fn zeroed_data() {
        crate::test_logger();

        let storage: PduStorage<1, { PduStorage::element_size(8) }> = PduStorage::new();

        let (_tx, _rx, pdu_loop) = storage.try_split().unwrap();

        let mut frame = pdu_loop.alloc_frame().expect("Allocate first frame");

        frame
            .push_pdu(Command::bwr(0x1000).into(), [0xaa, 0xbb, 0xcc, 0xdd], None)
            .unwrap();

        // Drop frame future to reset its state to `FrameState::None`
        drop(frame.mark_sendable(&pdu_loop, Duration::MAX, usize::MAX));

        let mut frame = pdu_loop.alloc_frame().expect("Allocate second frame");

        const LEN: usize = 8;

        frame.push_pdu(Command::Nop, (), Some(LEN as u16)).unwrap();

        let pdu_start = EthernetFrame::<&[u8]>::header_len()
            + EthercatFrameHeader::header_len()
            + PduHeader::PACKED_LEN;

        let frame = frame.mark_sendable(&pdu_loop, Duration::MAX, usize::MAX);

        // 10 byte PDU header, 8 byte payload, 2 byte WKC
        assert_eq!(
            // Skip all headers
            &frame.buf()[pdu_start..],
            // PDU payload plus working counter
            &[0u8; { LEN + 2 }]
        );
    }

    #[test]
    fn no_spare_frames() {
        crate::test_logger();

        const NUM_FRAMES: usize = 16;
        const DATA: usize = PduStorage::element_size(128);

        let storage: PduStorage<NUM_FRAMES, DATA> = PduStorage::new();
        let s = storage.as_ref();

        for _ in 0..NUM_FRAMES {
            assert!(s.alloc_frame().is_ok());
        }

        assert!(s.alloc_frame().is_err());
    }
}
