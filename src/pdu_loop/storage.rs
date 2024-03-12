use super::{pdu_rx::PduRx, pdu_tx::PduTx};
use crate::{
    command::Command,
    error::{Error, PduError},
    fmt,
    pdu_loop::{
        frame_element::{
            created_frame::CreatedFrame, receiving_frame::ReceivingFrame, FrameBox, FrameElement,
        },
        pdu_flags::PduFlags,
    },
    PduLoop,
};
use atomic_waker::AtomicWaker;
use core::{
    cell::UnsafeCell,
    marker::PhantomData,
    mem::MaybeUninit,
    ptr::NonNull,
    sync::atomic::{AtomicBool, AtomicU16, AtomicU8, Ordering},
};

const PDU_UNUSED_SENTINEL: u16 = u16::MAX;
const PDU_SLOTS: usize = 256;
/// Smallest frame size with a data payload of 0 length
const MIN_DATA: usize = FrameBox::ethernet_buf_len(&PduFlags::const_default());

/// Stores PDU frames that are currently being prepared to send, in flight, or being received and
/// processed.
///
/// The number of storage elements `N` must be a power of 2.
pub struct PduStorage<const N: usize, const DATA: usize> {
    frames: UnsafeCell<MaybeUninit<[FrameElement<DATA>; N]>>,
    pdu_states: [AtomicU16; PDU_SLOTS],
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
    pub const fn element_size(data_len: usize) -> usize {
        MIN_DATA + data_len
    }
}

impl<const N: usize, const DATA: usize> PduStorage<N, DATA> {
    /// Create a new `PduStorage` instance.
    ///
    /// The number of storage elements `N` must be a power of 2.
    pub const fn new() -> Self {
        // MSRV: Make `N` a `u8` when `generic_const_exprs` is stablised
        // If possible, try using `NonZeroU8`.
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

        // MSRV: When `array::from_fn` is const-stabilised
        // let pdu_states = array::from_fn(|_| AtomicU16::new(PDU_UNUSED_SENTINEL))
        // SAFETY: `AtomicU16` has the same underlying representation as `u16`
        let pdu_states = unsafe {
            core::mem::transmute::<[u16; PDU_SLOTS], [AtomicU16; PDU_SLOTS]>(
                [PDU_UNUSED_SENTINEL; PDU_SLOTS],
            )
        };

        Self {
            frames,
            frame_idx: AtomicU8::new(0),
            pdu_idx: AtomicU8::new(0),
            pdu_states,
            is_split: AtomicBool::new(false),
            tx_waker: AtomicWaker::new(),
        }
    }

    /// Create a PDU loop backed by this storage.
    ///
    /// Returns a TX and RX driver, and a handle to the PDU loop. This method will return an error
    /// if called more than once.
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
            num_frames: N,
            frame_data_len: DATA,
            frame_idx: &self.frame_idx,
            pdu_idx: &self.pdu_idx,
            pdu_states: &self.pdu_states,
            tx_waker: &self.tx_waker,
            _lifetime: PhantomData,
        }
    }
}

#[derive(Debug, Clone)]
pub(in crate::pdu_loop) struct PduStorageRef<'sto> {
    pub frames: NonNull<FrameElement<0>>,
    pub num_frames: usize,
    pub frame_data_len: usize,
    frame_idx: &'sto AtomicU8,
    pdu_idx: &'sto AtomicU8,
    pdu_states: &'sto [AtomicU16],
    pub tx_waker: &'sto AtomicWaker,
    _lifetime: PhantomData<&'sto ()>,
}

impl<'sto> PduStorageRef<'sto> {
    /// Allocate a PDU frame with the given command and data length.
    pub(in crate::pdu_loop) fn alloc_frame(
        &self,
        command: Command,
        data_length: u16,
    ) -> Result<CreatedFrame<'sto>, Error> {
        let data_length_usize = usize::from(data_length);

        if data_length_usize > self.frame_data_len {
            return Err(PduError::TooLong.into());
        }

        let mut search = 0;

        // Find next frame that is not currently in use.
        let (frame, frame_idx) = loop {
            let frame_idx = self.frame_idx.fetch_add(1, Ordering::Relaxed) % self.num_frames as u8;

            fmt::trace!("Try to allocate frame {}", frame_idx);

            // Claim frame so it is no longer free and can be used. It must be claimed before
            // initialisation to avoid race conditions with other threads potentially claiming the
            // same frame.
            let frame =
                unsafe { NonNull::new_unchecked(self.frame_at_index(usize::from(frame_idx))) };
            let frame = unsafe { FrameElement::claim_created(frame) };

            if let Ok(f) = frame {
                break (f, frame_idx);
            }

            search += 1;

            // Escape hatch: we'll only loop through the frame storage array twice to put an upper
            // bound on the number of times this loop can execute. It could be allowed to execute
            // indefinitely and rely on PDU future timeouts to cancel, but that seems brittle hence
            // this safety check.
            //
            // This can be mitigated by using a `RetryBehaviour` of `Count` or `Forever`.
            if search > self.num_frames * 2 {
                // We've searched twice and found no free slots. This means the application should
                // either slow down its packet sends, or increase `N` in `PduStorage` as there
                // aren't enough slots to hold all in-flight packets.
                fmt::error!("No available frames in {} slots", self.num_frames);

                return Err(PduError::SwapState.into());
            }
        };

        let pdu_idx = self.pdu_idx.fetch_add(1, Ordering::Relaxed);

        // Establish mapping between this PDU index and the Ethernet frame it's being put in
        self.pdu_states[usize::from(pdu_idx)]
            .compare_exchange(
                PDU_UNUSED_SENTINEL,
                u16::from(frame_idx),
                Ordering::Acquire,
                Ordering::Relaxed,
            )
            // TODO: Better error to explain that the requested PDU is already in flight
            .expect("In use!");

        let inner = FrameBox::init(frame, command, pdu_idx, data_length, self.frame_data_len)?;

        Ok(CreatedFrame { inner })
    }

    /// Updates state from SENDING -> RX_BUSY
    pub(in crate::pdu_loop) fn claim_receiving(&self, idx: u8) -> Option<ReceivingFrame<'sto>> {
        let idx = usize::from(idx);

        if idx >= self.num_frames {
            return None;
        }

        fmt::trace!("--> Claim receiving {:#04x}", idx);

        let frame = unsafe { NonNull::new_unchecked(self.frame_at_index(idx)) };
        let frame = unsafe { FrameElement::claim_receiving(frame)? };

        Some(ReceivingFrame {
            inner: FrameBox::new(frame),
        })
    }

    /// Retrieve a frame at the given index.
    ///
    /// # Safety
    ///
    /// If the given index is greater than the value in `PduStorage::N`, this will return garbage
    /// data off the end of the frame element buffer.
    pub(in crate::pdu_loop) unsafe fn frame_at_index(&self, idx: usize) -> *mut FrameElement<0> {
        let align = core::mem::align_of::<FrameElement<0>>();
        let size = core::mem::size_of::<FrameElement<0>>() + self.frame_data_len;

        let stride = core::alloc::Layout::from_size_align_unchecked(size, align)
            .pad_to_align()
            .size();

        self.frames.as_ptr().byte_add(idx * stride)
    }
}

unsafe impl<'sto> Send for PduStorageRef<'sto> {}
unsafe impl<'sto> Sync for PduStorageRef<'sto> {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pdu_loop::frame_element::FrameState;

    #[test]
    fn zeroed_data() {
        let storage: PduStorage<1, { PduStorage::element_size(8) }> = PduStorage::new();
        let s = storage.as_ref();

        let mut frame = s
            .alloc_frame(Command::fpwr(0x1234, 0x5678).into(), 4)
            .unwrap();

        frame.buf_mut().copy_from_slice(&[0xaa, 0xbb, 0xcc, 0xdd]);

        // Manually reset frame state so it can be reused.
        unsafe { FrameElement::set_state(frame.inner.frame, FrameState::None) };

        let mut frame = s
            .alloc_frame(Command::fpwr(0x1234, 0x5678).into(), 8)
            .unwrap();

        assert_eq!(frame.buf_mut(), &[0u8; 8]);
    }

    #[test]
    fn no_spare_frames() {
        let _ = env_logger::builder().is_test(true).try_init();

        const NUM_FRAMES: usize = 16;

        let storage: PduStorage<NUM_FRAMES, { PduStorage::element_size(128) }> = PduStorage::new();
        let s = storage.as_ref();

        for _ in 0..NUM_FRAMES {
            assert!(s.alloc_frame(Command::lwr(0x1234).into(), 128).is_ok());
        }

        assert!(s.alloc_frame(Command::lwr(0x1234).into(), 128).is_err());
    }

    #[test]
    fn too_long() {
        let _ = env_logger::builder().is_test(true).try_init();

        const NUM_FRAMES: usize = 16;

        let storage: PduStorage<NUM_FRAMES, { PduStorage::element_size(128) }> = PduStorage::new();
        let s = storage.as_ref();

        assert!(s.alloc_frame(Command::lwr(0x1234).into(), 129).is_err());
    }
}
