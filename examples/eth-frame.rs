//! Testing some ideas around storing multiple PDUs in a single Ethernet frame.

use ethercrab::{
    error::{Error, PduError},
    Command, EtherCrabWireWrite,
};
use smoltcp::wire::{EthernetAddress, EthernetFrame, EthernetProtocol};
use std::{
    cell::UnsafeCell,
    marker::PhantomData,
    mem::MaybeUninit,
    ptr::{addr_of, addr_of_mut, NonNull},
    sync::atomic::{AtomicU16, AtomicU8, AtomicUsize, Ordering},
    task::Waker,
};

#[atomic_enum::atomic_enum]
enum FrameState {
    // MUST remain 0x00 as this state represents the result of `MaybeUninit::zeroed()`.
    /// The frame is available ready to be claimed.
    None = 0,
    /// The frame is claimed with a zeroed data buffer and can be filled with command, data, etc
    /// ready for sending.
    Created = 1,
    /// The frame has been populated with data and is ready to send when the TX loop next runs.
    Sendable = 2,
    /// The frame is being sent over the network interface.
    Sending = 3,
    /// The frame was successfully sent, and is now waiting for a response from the network.
    Sent = 4,
    /// A frame response has been received and validation/parsing is in progress.
    RxBusy = 5,
    /// Frame response parsing is complete and the returned data is now stored in the frame. The
    /// frame and its data is ready to be returned in `Poll::Ready` of [`ReceiveFrameFut`].
    RxDone = 6,
    /// The frame TX/RX is complete, but the frame memory is still held by calling code.
    RxProcessing = 7,
}

#[repr(C)]
struct FrameElement<const MTU: usize> {
    state: AtomicFrameState,
    /// Number of bytes consumed by this frame.
    bytes_used: AtomicU16,
    // MUST be last so we can access other fields on `NonNull<FrameElement<0>>` safely.
    /// Ethernet frame data payload
    data: [u8; MTU],
}

impl<const MTU: usize> FrameElement<MTU> {
    /// Atomically swap the frame state from `from` to `to`.
    ///
    /// If the frame is not currently in the given `from` state, this method will return an error
    /// with the actual current frame state.
    unsafe fn swap_state(
        this: NonNull<FrameElement<MTU>>,
        from: FrameState,
        to: FrameState,
    ) -> Result<NonNull<FrameElement<MTU>>, FrameState> {
        let fptr = this.as_ptr();

        (*addr_of_mut!((*fptr).state)).compare_exchange(
            from,
            to,
            Ordering::AcqRel,
            Ordering::Relaxed,
        )?;

        Ok(this)
    }

    /// Pointer to entire ethernet frame buffer including header
    unsafe fn ethernet_frame_ptr(this: NonNull<FrameElement<MTU>>) -> NonNull<u8> {
        let buf_ptr: *mut [u8; MTU] = unsafe { addr_of_mut!((*this.as_ptr()).data) };
        let buf_ptr: *mut u8 = buf_ptr.cast();
        NonNull::new_unchecked(buf_ptr)
    }

    /// Attempt to clame a frame element as CREATED. Succeeds if the selected FrameElement is
    /// currently in the NONE state.
    pub unsafe fn claim_created(
        this: NonNull<FrameElement<MTU>>,
    ) -> Result<NonNull<FrameElement<MTU>>, PduError> {
        // SAFETY: We atomically ensure the frame is currently available to use which guarantees no
        // other thread could take it from under our feet.
        //
        // It is imperative that we check the existing state when claiming a frame as created. It
        // matters slightly less for all other state transitions because once we have a created
        // frame nothing else is able to take it unless it is put back into the `None` state.
        Self::swap_state(this, FrameState::None, FrameState::Created).map_err(|e| {
            log::debug!(
                "Failed to claim frame: status is {:?}, expected {:?}",
                e,
                FrameState::None
            );

            PduError::SwapState
        })
    }

    fn init(this: NonNull<FrameElement<0>>, mtu: usize) {
        let mut f = unsafe {
            let ptr = FrameElement::ethernet_frame_ptr(this);

            let data = core::slice::from_raw_parts_mut(ptr.as_ptr(), mtu);

            EthernetFrame::new_checked(data).expect("Frame too short")
        };

        f.set_src_addr(EthernetAddress::from_bytes(&[
            0x10, 0x10, 0x10, 0x10, 0x10, 0x10,
        ]));
        f.set_dst_addr(EthernetAddress::BROADCAST);
        f.set_ethertype(EthernetProtocol::Unknown(0x88a4));

        unsafe { &*addr_of!((*this.as_ptr()).bytes_used) }.store(
            EthernetFrame::<&[u8]>::header_len() as u16,
            Ordering::Release,
        );
    }
}

struct FrameStorage<const N: usize, const MTU: usize> {
    frames: UnsafeCell<[FrameElement<MTU>; N]>,
    wakers: UnsafeCell<[Option<Waker>; N]>,
    /// Index into `frames`.
    frame_idx: AtomicUsize,
    /// EtherCAT PDU index.
    pdu_idx: AtomicU8,
}

impl<const N: usize, const MTU: usize> FrameStorage<N, MTU> {
    const fn new() -> Self {
        // MSRV: Make `N` a `u8` when `generic_const_exprs` is stablised
        // If possible, try using `NonZeroU8`.
        assert!(
            N <= u8::MAX as usize,
            "PDU indices range from 0-255 so `N` may only be in this range."
        );
        assert!(N > 0, "Storage must contain at least one element.");

        // SAFETY: Waker must be explicitly set to at least `None` before being used.
        let wakers = UnsafeCell::new(unsafe { MaybeUninit::zeroed().assume_init() });

        // SAFETY: Frames must be initialised on first use, by checking for `FrameState::Uninit`.
        // `Uninit` MUST equal zero, which means this `MaybeUninit::zeroed()` will put the frame in
        // the correct state. Any other fields MUST NOT be accessed before the frame state is at
        // least `None`.
        let mut frames = UnsafeCell::new(unsafe { MaybeUninit::zeroed().assume_init() });

        Self {
            frames,
            wakers,
            frame_idx: AtomicUsize::new(0),
            pdu_idx: AtomicU8::new(0),
        }
    }

    fn as_ref(&self) -> FrameStorageRef {
        FrameStorageRef {
            frames: unsafe { NonNull::new_unchecked(self.frames.get().cast()) },
            num_frames: N,
            mtu: MTU,
            frame_idx: &self.frame_idx,
            pdu_idx: &self.pdu_idx,
            // tx_waker: &self.tx_waker,
            _lifetime: PhantomData,
        }
    }
}

/// Reference to storage, with const generics erased
struct FrameStorageRef<'sto> {
    frames: NonNull<FrameElement<0>>,
    /// `N`.
    num_frames: usize,
    /// `MTU`.
    mtu: usize,
    /// Ethernet frame index.
    frame_idx: &'sto AtomicUsize,
    /// EtherCAT frame index.
    pdu_idx: &'sto AtomicU8,
    // TODO: Waker
    // pub tx_waker: &'sto AtomicWaker,
    _lifetime: PhantomData<&'sto ()>,
}

impl<'sto> FrameStorageRef<'sto> {
    fn next_frame_index(&self) -> usize {
        self.frame_idx
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                if current == self.num_frames {
                    Some(0)
                } else {
                    Some(current + 1)
                }
            })
            // This should be unreachable because the closure above always returns `Some`.
            .expect("Increment index")
    }

    fn next_pdu_index(&self) -> u8 {
        self.pdu_idx
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                if current == self.num_frames as u8 {
                    Some(0)
                } else {
                    Some(current + 1)
                }
            })
            // This should be unreachable because the closure above always returns `Some`.
            .expect("Increment index")
    }

    /// Retrieve a frame at the given index.
    ///
    /// # Safety
    ///
    /// If the given index is greater than the value in `PduStorage::N`, this will return garbage
    /// data off the end of the frame element buffer.
    unsafe fn frame_at_index(&self, idx: usize) -> *mut FrameElement<0> {
        let align = core::mem::align_of::<FrameElement<0>>();
        let size = core::mem::size_of::<FrameElement<0>>() + self.mtu;

        let stride = core::alloc::Layout::from_size_align_unchecked(size, align)
            .pad_to_align()
            .size();

        self.frames.as_ptr().byte_add(idx * stride)
    }

    fn alloc_frame(&self) -> Result<CreatedFrame, Error> {
        let mut search = 0;

        // Find next frame that is not currently in use.
        let frame = loop {
            let idx = self.next_frame_index();

            log::trace!("Try to allocate frame {:#04x}", idx);

            // Claim frame so it is no longer free and can be used. It must be claimed before
            // initialisation to avoid race conditions with other threads potentially claiming the
            // same frame.
            let frame = unsafe { NonNull::new_unchecked(self.frame_at_index(idx)) };
            let frame = unsafe { FrameElement::claim_created(frame) };

            if let Ok(f) = frame {
                break f;
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
                log::error!("No available frames in {} slots", self.num_frames);

                return Err(PduError::SwapState.into());
            }
        };

        // TODO: Move this block into the alloc_pdu method
        // // Initialise frame with EtherCAT header and zeroed data buffer.
        // unsafe {
        //     addr_of_mut!((*frame.as_ptr()).frame).write(PduFrame {
        //         index: idx_u8,
        //         waker: AtomicWaker::new(),
        //         command,
        //         flags: PduFlags::with_len(data_length),
        //         irq: 0,
        //         working_counter: 0,
        //     });

        //     let buf_ptr: *mut u8 = addr_of_mut!((*frame.as_ptr()).buffer).cast();
        //     buf_ptr.write_bytes(0x00, data_length_usize);
        // }

        // unsafe { FrameElement::reset_ethernet_header(frame) };

        FrameElement::<0>::init(frame, self.mtu);

        let inner = EthernetFrameBox {
            mtu: self.mtu,
            frame,
            _lifetime: PhantomData,
        };

        Ok(CreatedFrame { inner })
    }
}

pub struct CreatedFrame<'sto> {
    pub inner: EthernetFrameBox<'sto>,
}

pub struct EthernetFrameBox<'sto> {
    pub mtu: usize,
    pub frame: NonNull<FrameElement<0>>,
    pub _lifetime: PhantomData<&'sto mut FrameElement<0>>,
}

impl<'sto> EthernetFrameBox<'sto> {
    // unsafe fn buf_mut(&mut self) -> &mut [u8] {
    //     let ptr = FrameElement::<0>::buf_ptr(self.frame);
    //     core::slice::from_raw_parts_mut(ptr.as_ptr(), self.buf_len)
    // }

    unsafe fn frame(&self) -> &FrameElement<0> {
        unsafe { &*addr_of!((*self.frame.as_ptr())) }
    }

    /// Reserve a section of the ethernet frame buffer to insert a PDU into.
    unsafe fn alloc_pdu(&self, len: u16) -> Result<(), Error> {
        let frame = self.frame();

        let start = frame
            .bytes_used
            .fetch_update(Ordering::Release, Ordering::Acquire, |current| {
                if usize::from(current + len) > self.mtu {
                    None
                } else {
                    Some(current + len)
                }
            })
            // TODO: If PDU won't fit, error instead of panic
            .expect("Won't fit");

        // TODO: Buffer WITHOUT ethernet header
        // let buf = {
        //     let ptr = FrameElement::<0>::buf_ptr(self.frame);

        //     core::slice::from_raw_parts_mut(ptr.as_ptr().add(usize::from(start)), usize::from(len))
        // };

        // TODO: Return header and buffer in a struct that can set command, payload,
        // etc. I think this struct should be the future that we await, right? Need to store
        // original buffer start/len so we can read data out of the received packet.

        Ok(())
    }
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    // // TODO: storage.claim_sending();
    // let mut slot = FrameElement::new();

    // let c = Command::brd(0x1000);

    // let f1 = slot.push_pdu(c)?;

    // slot.submit()?;

    // let res = f1.await;

    // First is N, second is MTU
    let storage = FrameStorage::<16, 16>::new();

    let storage_ref = storage.as_ref();

    let ethernet_frame = storage_ref.alloc_frame().expect("Alloc ethernet frame");

    // TODO: `inner` should be private or the typestate is useless lol
    let f1 = unsafe { ethernet_frame.inner.alloc_pdu(2) }.expect("F1");
    log::info!("F1");
    let f2 = unsafe { ethernet_frame.inner.alloc_pdu(8) }.expect("F2");
    log::info!("F2");
}
