use crate::{command::Command, error::PduError, fmt, pdu_loop::pdu_flags::PduFlags};
use atomic_waker::AtomicWaker;
use core::{
    fmt::Debug,
    marker::PhantomData,
    ptr::{addr_of, addr_of_mut, NonNull},
    sync::atomic::Ordering,
    task::Waker,
};

pub mod created_frame;
pub mod received_frame;
pub mod receiving_frame;
pub mod sendable_frame;

/// Frame state.
#[atomic_enum::atomic_enum]
#[derive(PartialEq, Default)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum FrameState {
    // SAFETY: Because we create a bunch of `Frame`s with `MaybeUninit::zeroed`, the `None` state
    // MUST be equal to zero. All other fields in `Frame` are overridden in `replace`, so there
    // should be no UB there.
    /// The frame is available ready to be claimed.
    #[default]
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

#[derive(Debug, Default)]
pub struct PduFrame {
    pub index: u8,
    pub command: Command,
    pub flags: PduFlags,
    pub irq: u16,
    pub working_counter: u16,

    pub waker: AtomicWaker,
}

/// An individual frame state, PDU header config, and data buffer.
///
/// # A frame's journey
///
/// The following flowchart describes a `FrameElement`'s state changes during its use:
///
/// <img alt="A flowchart showing the different state transitions of FrameElement" src="https://mermaid.ink/svg/pako:eNqdUztv2zAQ_isHTgngtLuGDLVadGkQ2E7bQYBxEc82YYoU-LBsJPnvPVLMy_JULaR05PfS3ZNorSRRiY22Q7tDF2BVNwb4-eGwo2XAQFV1Zw3Bzc3tcyNQa9uuN6l4dd00Jh8D5cHYAejY6ujVgfQJrrYRHZpAJOHxBBhsp1rwCfAa8IBK46MmCBZaxlRmC0lKI54_Mc8d8Sqnkkohq-ptHzW_wX39wChdh0bOQGLA_wBrRDb3pUO3X3syMsnMVlc_v9_x8gf3BKu_oK3tz-Uuy_kpxWslc5TbkOA92AM5MBQG6_ZTuJTMZbhUGRUvCp6jljh9D9nCLCfroZdx7Y7Zwlyj6koZ0JcLDHRuZHH8Fv1pyjt-L7S_UStOWVnztXe2Je_H39j1mgIx3eIVPkNUVc60iJRZUA5zlDPw1k111Nx7l3TU7z05gsQQXSKdf2gnTsDkzoye2KzvreFN6owp0f2bhUt079VC-omG-18mPYMKu9HOm3uSxbx0tmfPZ7xptMRMdOQ6VJIn8SmxNyLsiEFExVvJqTWiMS98DmOwy5NpRRVcpJmIPZuhWuGWMUW1Qe35K0kVrPs1jnae8Jd_545fZQ" style="background: white; max-height: 800px" />
///
/// Source (MermaidJS):
///
/// ```mermaid
/// flowchart TD
///    FrameState::None -->|"alloc_frame()\nFrame is now exclusively (guaranteed by atomic state) available to calling code"| FrameState::Created
///    FrameState::Created -->|populate PDU command, data| FrameState::Created
///    FrameState::Created -->|"frame.mark_sendable()\nTHEN\nWake TX loop"| FrameState::Sendable
///    FrameState::Sendable -->|TX loop sends over network| FrameState::Sending
///    FrameState::Sending -->|"RX loop receives frame, calls pdu_rx()\nClaims frame as receiving"| FrameState::RxBusy
///    FrameState::RxBusy -->|"Validation/processing complete\nReceivingFrame::mark_received()\nWake frame waker"| FrameState::RxDone
///    FrameState::RxDone -->|"Wake future\nCalling code can now use response data"| FrameState::RxProcessing
///    FrameState::RxProcessing -->|"Calling code is done with frame\nReceivedFrame::drop()"| FrameState::None
///    ```
#[derive(Debug)]
#[repr(C)]
pub struct FrameElement<const N: usize> {
    pub frame: PduFrame,
    status: AtomicFrameState,
    pub buffer: [u8; N],
}

impl<const N: usize> Default for FrameElement<N> {
    fn default() -> Self {
        Self {
            frame: Default::default(),
            status: AtomicFrameState::new(FrameState::None),
            buffer: [0; N],
        }
    }
}

impl<const N: usize> FrameElement<N> {
    unsafe fn buf_ptr(this: NonNull<FrameElement<N>>) -> NonNull<u8> {
        let buf_ptr: *mut [u8; N] = unsafe { addr_of_mut!((*this.as_ptr()).buffer) };
        let buf_ptr: *mut u8 = buf_ptr.cast();
        NonNull::new_unchecked(buf_ptr)
    }

    /// Set the frame's state without checking its current state.
    pub(in crate::pdu_loop) unsafe fn set_state(this: NonNull<FrameElement<N>>, state: FrameState) {
        let fptr = this.as_ptr();

        (*addr_of_mut!((*fptr).status)).store(state, Ordering::Release);
    }

    /// Atomically swap the frame state from `from` to `to`.
    ///
    /// If the frame is not currently in the given `from` state, this method will return an error
    /// with the actual current frame state.
    unsafe fn swap_state(
        this: NonNull<FrameElement<N>>,
        from: FrameState,
        to: FrameState,
    ) -> Result<NonNull<FrameElement<N>>, FrameState> {
        let fptr = this.as_ptr();

        (*addr_of_mut!((*fptr).status)).compare_exchange(
            from,
            to,
            Ordering::AcqRel,
            Ordering::Relaxed,
        )?;

        Ok(this)
    }

    /// Attempt to clame a frame element as CREATED. Succeeds if the selected FrameElement is
    /// currently in the NONE state.
    pub unsafe fn claim_created(
        this: NonNull<FrameElement<N>>,
    ) -> Result<NonNull<FrameElement<N>>, PduError> {
        // SAFETY: We atomically ensure the frame is currently available to use which guarantees no
        // other thread could take it from under our feet.
        //
        // It is imperative that we check the existing state when claiming a frame as created. It
        // matters slightly less for all other state transitions because once we have a created
        // frame nothing else is able to take it unless it is put back into the `None` state.
        Self::swap_state(this, FrameState::None, FrameState::Created).map_err(|e| {
            fmt::debug!(
                "Failed to claim frame: status is {:?}, expected {:?}",
                e,
                FrameState::None
            );

            PduError::SwapState
        })
    }

    pub unsafe fn claim_sending(
        this: NonNull<FrameElement<N>>,
    ) -> Option<NonNull<FrameElement<N>>> {
        Self::swap_state(this, FrameState::Sendable, FrameState::Sending).ok()
    }

    pub unsafe fn claim_receiving(
        this: NonNull<FrameElement<N>>,
    ) -> Option<NonNull<FrameElement<N>>> {
        Self::swap_state(this, FrameState::Sent, FrameState::RxBusy)
            .map_err(|actual_state| {
                fmt::error!(
                    "Failed to claim receiving frame: expected state {:?}, but got {:?}",
                    FrameState::Sent,
                    actual_state
                );
            })
            .ok()
    }
}

/// Frame data common to all typestates.
pub struct FrameBox<'sto> {
    pub frame: NonNull<FrameElement<0>>,
    pub _lifetime: PhantomData<&'sto mut FrameElement<0>>,
}

impl<'sto> Debug for FrameBox<'sto> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let (frame, data) = unsafe { self.frame_and_buf() };

        f.debug_struct("FrameBox")
            .field("state", unsafe {
                &(*addr_of!((*self.frame.as_ptr()).status))
            })
            .field("frame", frame)
            .field("data_hex", &format_args!("{:02x?}", data))
            .finish()
    }
}

impl<'sto> FrameBox<'sto> {
    unsafe fn replace_waker(&self, waker: &Waker) {
        (*addr_of!((*self.frame.as_ptr()).frame.waker)).register(waker);
    }

    unsafe fn wake(&self) {
        (*addr_of!((*self.frame.as_ptr()).frame.waker)).wake()
    }

    unsafe fn frame(&self) -> &PduFrame {
        unsafe { &*addr_of!((*self.frame.as_ptr()).frame) }
    }

    unsafe fn frame_mut(&mut self) -> &mut PduFrame {
        unsafe { &mut *addr_of_mut!((*self.frame.as_ptr()).frame) }
    }

    unsafe fn buf_len(&self) -> usize {
        usize::from(self.frame().flags.len())
    }

    unsafe fn frame_and_buf(&self) -> (&PduFrame, &[u8]) {
        let buf_ptr = unsafe { addr_of!((*self.frame.as_ptr()).buffer).cast::<u8>() };
        let buf = unsafe { core::slice::from_raw_parts(buf_ptr, self.buf_len()) };
        let frame = unsafe { &*addr_of!((*self.frame.as_ptr()).frame) };
        (frame, buf)
    }

    unsafe fn buf_mut(&mut self) -> &mut [u8] {
        let ptr = FrameElement::<0>::buf_ptr(self.frame);
        core::slice::from_raw_parts_mut(ptr.as_ptr(), self.buf_len())
    }
}
