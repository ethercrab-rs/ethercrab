use super::{pdu_rx::PduRx, pdu_tx::PduTx};
use crate::{
    command::Command,
    error::{Error, PduError},
    fmt,
    pdu_loop::{
        frame_element::{
            created_frame::CreatedFrame, receiving_frame::ReceivingFrame, FrameBox, FrameElement,
            PduFrame,
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
    ptr::{addr_of_mut, NonNull},
    sync::atomic::{AtomicBool, AtomicU8, Ordering},
};

/// Stores PDU frames that are currently being prepared to send, in flight, or being received and
/// processed.
///
/// The number of storage elements `N` must be a power of 2.
pub struct PduStorage<const N: usize, const DATA: usize> {
    frames: UnsafeCell<MaybeUninit<[FrameElement<DATA>; N]>>,
    idx: AtomicU8,
    is_split: AtomicBool,
    /// A waker used to wake up the TX task when a new frame is ready to be sent.
    pub(in crate::pdu_loop) tx_waker: AtomicWaker,
}

unsafe impl<const N: usize, const DATA: usize> Sync for PduStorage<N, DATA> {}

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
            idx: AtomicU8::new(0),
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
            idx: &self.idx,
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
    /// EtherCAT frame index.
    ///
    /// This is incremented atomically to allow simultaneous allocation of available frame elements.
    idx: &'sto AtomicU8,
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
        let (frame, idx_u8) = loop {
            let idx_u8 = self.idx.fetch_add(1, Ordering::Relaxed) % self.num_frames as u8;

            let idx = usize::from(idx_u8);

            fmt::trace!("Try to allocate frame #{}", idx);

            // Claim frame so it is no longer free and can be used. It must be claimed before
            // initialisation to avoid race conditions with other threads potentially claiming the
            // same frame.
            let frame = unsafe { NonNull::new_unchecked(self.frame_at_index(idx)) };
            let frame = unsafe { FrameElement::claim_created(frame) };

            if let Ok(f) = frame {
                break (f, idx_u8);
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

        // Initialise frame with EtherCAT header and zeroed data buffer.
        unsafe {
            addr_of_mut!((*frame.as_ptr()).frame).write(PduFrame {
                index: idx_u8,
                waker: AtomicWaker::new(),
                command,
                flags: PduFlags::with_len(data_length),
                irq: 0,
                working_counter: 0,
            });

            let buf_ptr: *mut u8 = addr_of_mut!((*frame.as_ptr()).buffer).cast();
            buf_ptr.write_bytes(0x00, data_length_usize);
        }

        Ok(CreatedFrame {
            inner: FrameBox {
                frame,
                _lifetime: PhantomData,
            },
        })
    }

    /// Updates state from SENDING -> RX_BUSY
    pub(in crate::pdu_loop) fn claim_receiving(&self, idx: u8) -> Option<ReceivingFrame<'sto>> {
        let idx = usize::from(idx);

        if idx >= self.num_frames {
            return None;
        }

        fmt::trace!("Receiving frame {}", idx);

        let frame = unsafe { NonNull::new_unchecked(self.frame_at_index(idx)) };
        let frame = unsafe { FrameElement::claim_receiving(frame)? };

        Some(ReceivingFrame {
            inner: FrameBox {
                frame,
                _lifetime: PhantomData,
            },
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
        let storage: PduStorage<1, 8> = PduStorage::new();
        let s = storage.as_ref();

        let mut frame = s
            .alloc_frame(Command::Write(Command::fpwr(0x1234, 0x5678)), 4)
            .unwrap();

        frame.buf_mut().copy_from_slice(&[0xaa, 0xbb, 0xcc, 0xdd]);

        // Manually reset frame state so it can be reused.
        unsafe { FrameElement::set_state(frame.inner.frame, FrameState::None) };

        let mut frame = s
            .alloc_frame(Command::Write(Command::fpwr(0x1234, 0x5678)), 8)
            .unwrap();

        assert_eq!(frame.buf_mut(), &[0u8; 8]);
    }

    #[test]
    fn no_spare_frames() {
        let _ = env_logger::builder().is_test(true).try_init();

        const NUM_FRAMES: usize = 16;

        let storage: PduStorage<NUM_FRAMES, 128> = PduStorage::new();
        let s = storage.as_ref();

        for _ in 0..NUM_FRAMES {
            assert!(s
                .alloc_frame(Command::Write(Command::lwr(0x1234)), 128)
                .is_ok());
        }

        assert!(s
            .alloc_frame(Command::Write(Command::lwr(0x1234)), 128)
            .is_err());
    }

    #[test]
    fn too_long() {
        let _ = env_logger::builder().is_test(true).try_init();

        const NUM_FRAMES: usize = 16;

        let storage: PduStorage<NUM_FRAMES, 128> = PduStorage::new();
        let s = storage.as_ref();

        assert!(s
            .alloc_frame(Command::Write(Command::lwr(0x1234)), 129)
            .is_err());
    }
}
