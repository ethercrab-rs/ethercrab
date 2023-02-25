use crate::{
    command::Command,
    error::{Error, PduError},
    pdu_loop::{
        frame_element::{CreatedFrame, FrameBox, FrameElement, PduFrame, ReceivingFrame},
        pdu_flags::PduFlags,
    },
};
use core::{
    cell::UnsafeCell,
    marker::PhantomData,
    mem::MaybeUninit,
    ptr::{addr_of_mut, NonNull},
    sync::atomic::{AtomicU8, Ordering},
};

/// Stores PDU frames that are currently being prepared to send, in flight, or being received and
/// processed.
pub struct PduStorage<const N: usize, const DATA: usize> {
    frames: UnsafeCell<MaybeUninit<[FrameElement<DATA>; N]>>,
}

unsafe impl<const N: usize, const DATA: usize> Sync for PduStorage<N, DATA> {}

impl<const N: usize, const DATA: usize> PduStorage<N, DATA> {
    /// Create a new `PduStorage` instance.
    pub const fn new() -> Self {
        // MSRV: Make `N` a `u8` when `generic_const_exprs` is stablised
        assert!(
            N <= u8::MAX as usize,
            "Packet indexes are u8s, so cache array cannot be any bigger than u8::MAX"
        );

        let frames = UnsafeCell::new(unsafe { MaybeUninit::zeroed().assume_init() });

        Self { frames }
    }

    /// Get a reference to this `PduStorage` with erased lifetimes.
    pub const fn as_ref(&self) -> PduStorageRef<'_> {
        PduStorageRef {
            frames: unsafe { NonNull::new_unchecked(self.frames.get().cast()) },
            num_frames: N,
            frame_data_len: DATA,
            idx: AtomicU8::new(0),
            _lifetime: PhantomData,
        }
    }
}

#[derive(Debug)]
pub struct PduStorageRef<'a> {
    pub frames: NonNull<FrameElement<0>>,
    pub num_frames: usize,
    pub frame_data_len: usize,
    idx: AtomicU8,
    _lifetime: PhantomData<&'a ()>,
}

impl<'a> PduStorageRef<'a> {
    pub fn alloc_frame(
        &self,
        command: Command,
        data_length: u16,
    ) -> Result<CreatedFrame<'a>, Error> {
        let data_length_usize = usize::from(data_length);

        if data_length_usize > self.frame_data_len {
            return Err(PduError::TooLong.into());
        }

        let idx_u8 = self.idx.fetch_add(1, Ordering::AcqRel) % self.num_frames as u8;

        let idx = usize::from(idx_u8);

        log::trace!("Try to allocate frame #{idx}");

        let frame = unsafe { NonNull::new_unchecked(self.frame_at_index(idx)) };
        let frame = unsafe { FrameElement::claim_created(frame) }?;

        // Initialise frame
        unsafe {
            addr_of_mut!((*frame.as_ptr()).frame).write(PduFrame {
                len: data_length_usize,
                index: idx_u8,
                waker: spin::RwLock::new(None),
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
    pub fn get_receiving(&self, idx: u8) -> Option<ReceivingFrame<'a>> {
        let idx = usize::from(idx);

        if idx >= self.num_frames {
            return None;
        }

        log::trace!("Receiving frame {idx}");

        let frame = unsafe { NonNull::new_unchecked(self.frame_at_index(idx)) };
        let frame = unsafe { FrameElement::claim_receiving(frame)? };

        Some(ReceivingFrame {
            inner: FrameBox {
                frame,
                _lifetime: PhantomData,
            },
        })
    }

    pub(in crate::pdu_loop) unsafe fn frame_at_index(&self, idx: usize) -> *mut FrameElement<0> {
        let align = core::mem::align_of::<FrameElement<0>>();
        let size = core::mem::size_of::<FrameElement<0>>() + self.frame_data_len;

        let stride = core::alloc::Layout::from_size_align_unchecked(size, align)
            .pad_to_align()
            .size();

        // NIGHTLY: pointer_byte_offsets
        self.frames.as_ptr().byte_add(idx * stride)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pdu_loop::frame_element::FrameState;

    #[test]
    fn zeroed_data() {
        let storage: PduStorage<1, 8> = PduStorage::new();
        let s = storage.as_ref();

        let mut frame = s
            .alloc_frame(
                Command::Brd {
                    address: 0x1234,
                    register: 0x5678,
                },
                4,
            )
            .unwrap();

        frame.buf_mut().copy_from_slice(&[0xaa, 0xbb, 0xcc, 0xdd]);

        // Manually reset frame state so it can be reused.
        unsafe { FrameElement::set_state(frame.inner.frame, FrameState::None) };

        let mut frame = s
            .alloc_frame(
                Command::Brd {
                    address: 0x1234,
                    register: 0x5678,
                },
                8,
            )
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
            assert!(s.alloc_frame(Command::Lwr { address: 0x1234 }, 128).is_ok());
        }

        assert!(s
            .alloc_frame(Command::Lwr { address: 0x1234 }, 128)
            .is_err());
    }

    #[test]
    fn too_long() {
        let _ = env_logger::builder().is_test(true).try_init();

        const NUM_FRAMES: usize = 16;

        let storage: PduStorage<NUM_FRAMES, 128> = PduStorage::new();
        let s = storage.as_ref();

        assert!(s
            .alloc_frame(Command::Lwr { address: 0x1234 }, 129)
            .is_err());
    }
}
