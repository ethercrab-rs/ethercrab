use crate::{
    command::Command,
    eeprom::types::Flags,
    error::{Error, PduError},
    pdu_loop::frame_element::{FrameBox, FrameElement, ReceivingFrame},
};
use core::{
    cell::UnsafeCell,
    marker::PhantomData,
    mem::MaybeUninit,
    ptr::{addr_of_mut, NonNull},
    sync::atomic::{AtomicU8, Ordering},
};

use super::{
    frame_element::{CreatedFrame, PduFrame},
    pdu::PduFlags,
};

/// TODO: Docs
pub struct PduStorage<const N: usize, const DATA: usize> {
    /// TODO: Docs
    pub frames: UnsafeCell<MaybeUninit<[FrameElement<DATA>; N]>>,
}

unsafe impl<const N: usize, const DATA: usize> Sync for PduStorage<N, DATA> {}

impl<const N: usize, const DATA: usize> PduStorage<N, DATA> {
    /// TODO: Docs
    pub const fn new() -> Self {
        let frames = UnsafeCell::new(unsafe { MaybeUninit::zeroed().assume_init() });

        Self { frames }
    }

    /// TODO: Docs
    pub const fn as_ref<'a>(&'a self) -> PduStorageRef<'a> {
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
        let data_length = usize::from(data_length);

        if data_length > self.frame_data_len {
            return Err(PduError::TooLong.into());
        }

        let idx_u8 = self.idx.fetch_add(1, Ordering::AcqRel);

        let idx = usize::from(idx_u8) % self.num_frames;

        let frame = unsafe { NonNull::new_unchecked(self.frames.as_ptr().add(idx)) };
        let frame = unsafe { FrameElement::claim_created(frame) }?;

        // Initialise frame
        unsafe {
            addr_of_mut!((*frame.as_ptr()).frame).write(PduFrame {
                len: data_length,
                index: idx_u8,
                waker: spin::RwLock::new(None),
                command,
                flags: PduFlags::default(),
                irq: 0,
                working_counter: 0,
            });

            let buf_ptr = addr_of_mut!((*frame.as_ptr()).buffer);
            buf_ptr.write_bytes(0x00, data_length);
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

        let frame = unsafe { NonNull::new_unchecked(self.frames.as_ptr().add(idx)) };
        let frame = unsafe { FrameElement::claim_receiving(frame)? };

        Some(ReceivingFrame {
            inner: FrameBox {
                frame,
                _lifetime: PhantomData,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
