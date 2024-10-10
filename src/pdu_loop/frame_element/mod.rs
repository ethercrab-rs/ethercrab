pub mod created_frame;
mod frame_box;
pub mod received_frame;
pub mod receiving_frame;
pub mod sendable_frame;

use crate::{
    error::PduError, ethernet::EthernetFrame, fmt, pdu_loop::frame_header::EthercatFrameHeader,
};
use atomic_waker::AtomicWaker;
use core::{
    ptr::{addr_of, addr_of_mut, NonNull},
    sync::atomic::{AtomicU16, Ordering},
};
use frame_box::FrameBox;

/// A marker value for empty frames with no pushed PDUs.
///
/// The upper value must be non-zero for sentinel comparisons to work.
pub const FIRST_PDU_EMPTY: u16 = 0xff00;

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

/// An individual frame state, PDU header config, and data buffer.
///
/// # A frame's journey
///
// TODO: Update this journey! The current docs are out of date!
// The following flowchart describes a `FrameElement`'s state changes during its use:
//
// <img alt="A flowchart showing the different state transitions of FrameElement" src="https://mermaid.ink/svg/pako:eNqdUztv2zAQ_isHTgngtLuGDLVadGkQ2E7bQYBxEc82YYoU-LBsJPnvPVLMy_JULaR05PfS3ZNorSRRiY22Q7tDF2BVNwb4-eGwo2XAQFV1Zw3Bzc3tcyNQa9uuN6l4dd00Jh8D5cHYAejY6ujVgfQJrrYRHZpAJOHxBBhsp1rwCfAa8IBK46MmCBZaxlRmC0lKI54_Mc8d8Sqnkkohq-ptHzW_wX39wChdh0bOQGLA_wBrRDb3pUO3X3syMsnMVlc_v9_x8gf3BKu_oK3tz-Uuy_kpxWslc5TbkOA92AM5MBQG6_ZTuJTMZbhUGRUvCp6jljh9D9nCLCfroZdx7Y7Zwlyj6koZ0JcLDHRuZHH8Fv1pyjt-L7S_UStOWVnztXe2Je_H39j1mgIx3eIVPkNUVc60iJRZUA5zlDPw1k111Nx7l3TU7z05gsQQXSKdf2gnTsDkzoye2KzvreFN6owp0f2bhUt079VC-omG-18mPYMKu9HOm3uSxbx0tmfPZ7xptMRMdOQ6VJIn8SmxNyLsiEFExVvJqTWiMS98DmOwy5NpRRVcpJmIPZuhWuGWMUW1Qe35K0kVrPs1jnae8Jd_545fZQ" style="background: white; max-height: 800px" />
//
// Source (MermaidJS):
//
// ```mermaid
// flowchart TD
//    FrameState::None -->|"alloc_frame()\nFrame is now exclusively (guaranteed by atomic state) available to calling code"| FrameState::Created
//    FrameState::Created -->|populate PDU command, data| FrameState::Created
//    FrameState::Created -->|"frame.mark_sendable()\nTHEN\nWake TX loop"| FrameState::Sendable
//    FrameState::Sendable -->|TX loop sends over network| FrameState::Sending
//    FrameState::Sending -->|"RX loop receives frame, calls pdu_rx()\nClaims frame as receiving"| FrameState::RxBusy
//    FrameState::RxBusy -->|"Validation/processing complete\nReceivingFrame::mark_received()\nWake frame waker"| FrameState::RxDone
//    FrameState::RxDone -->|"Wake future\nCalling code can now use response data"| FrameState::RxProcessing
//    FrameState::RxProcessing -->|"Calling code is done with frame\nReceivedFrame::drop()"| FrameState::None
//    ```
#[derive(Debug)]
#[repr(C)]
pub struct FrameElement<const N: usize> {
    /// Ethernet frame index. Has nothing to do with PDU header index field.
    frame_index: u8,
    status: AtomicFrameState,
    waker: AtomicWaker,

    /// Keeps track of how much of the PDU data buffer has been consumed.
    pdu_payload_len: usize,

    // Atomic as we iterate over all `FrameElement`s and read this field when receiving a frame.
    /// Stores the PDU index of the first PDU written into this frame (if any).
    ///
    /// Used by the network RX code to do a linear search in the frame storage to find the storage
    /// behind the received frame.
    ///
    /// The lower byte stores the PDU index, the upper byte stores a sentinel used to signify
    /// whether the PDU has been set or not.
    first_pdu: AtomicU16,

    // MUST be the last element otherwise pointer arithmetic doesn't work for
    // `NonNull<FrameElement<0>>`.
    ethernet_frame: [u8; N],
}

impl<const N: usize> Default for FrameElement<N> {
    fn default() -> Self {
        Self {
            status: AtomicFrameState::new(FrameState::None),
            ethernet_frame: [0; N],
            frame_index: 0,
            pdu_payload_len: 0,
            first_pdu: AtomicU16::new(FIRST_PDU_EMPTY),
            waker: AtomicWaker::default(),
        }
    }
}

impl<const N: usize> FrameElement<N> {
    /// Get pointer to entire data: the Ethernet frame including header and all subsequent EtherCAT
    /// payload.
    unsafe fn ptr(this: NonNull<FrameElement<N>>) -> NonNull<u8> {
        let buf_ptr: *mut [u8; N] = unsafe { addr_of_mut!((*this.as_ptr()).ethernet_frame) };
        let buf_ptr: *mut u8 = buf_ptr.cast();
        NonNull::new_unchecked(buf_ptr)
    }

    /// Get pointer to EtherCAT frame payload. i.e. the buffer after the end of the EtherCAT frame
    /// header where all the PDUs (header and data) go.
    unsafe fn ethercat_payload_ptr(this: NonNull<FrameElement<N>>) -> NonNull<u8> {
        // MSRV: `feature(non_null_convenience)` when stabilised
        NonNull::new_unchecked(
            Self::ptr(this)
                .as_ptr()
                .byte_add(EthernetFrame::<&[u8]>::header_len())
                .byte_add(EthercatFrameHeader::header_len()), // .byte_add(PduFrame::header_len()),
        )
    }

    /// Set the frame's state without checking its current state.
    unsafe fn set_state(this: NonNull<FrameElement<N>>, state: FrameState) {
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
    unsafe fn claim_created(
        this: NonNull<FrameElement<N>>,
        frame_index: u8,
    ) -> Result<NonNull<FrameElement<N>>, PduError> {
        // SAFETY: We atomically ensure the frame is currently available to use which guarantees no
        // other thread could take it from under our feet.
        //
        // It is imperative that we check the existing state when claiming a frame as created. It
        // matters slightly less for all other state transitions because once we have a created
        // frame nothing else is able to take it unless it is put back into the `None` state.
        let this = Self::swap_state(this, FrameState::None, FrameState::Created).map_err(|e| {
            fmt::debug!(
                "Failed to claim frame: status is {:?}, expected {:?}",
                e,
                FrameState::None
            );

            PduError::SwapState
        })?;

        (*addr_of_mut!((*this.as_ptr()).frame_index)) = frame_index;
        (*addr_of_mut!((*this.as_ptr()).pdu_payload_len)) = 0;

        Ok(this)
    }

    unsafe fn claim_sending(this: NonNull<FrameElement<N>>) -> Option<NonNull<FrameElement<N>>> {
        Self::swap_state(this, FrameState::Sendable, FrameState::Sending).ok()
    }

    unsafe fn claim_receiving(this: NonNull<FrameElement<N>>) -> Option<NonNull<FrameElement<N>>> {
        Self::swap_state(this, FrameState::Sent, FrameState::RxBusy)
            .map_err(|actual_state| {
                fmt::error!(
                    "Failed to claim receiving frame {}: expected state {:?}, but got {:?}",
                    (*addr_of_mut!((*this.as_ptr()).frame_index)),
                    FrameState::Sent,
                    actual_state
                );
            })
            .ok()
    }

    unsafe fn frame_index(this: NonNull<FrameElement<0>>) -> u8 {
        *addr_of!((*this.as_ptr()).frame_index)
    }

    pub(in crate::pdu_loop) unsafe fn first_pdu_is(
        this: NonNull<FrameElement<0>>,
        search: u8,
    ) -> bool {
        let raw = (*addr_of!((*this.as_ptr()).first_pdu)).load(Ordering::Acquire);

        // Unused sentinel value occupies upper byte, so this equality will never hold for empty
        // frames
        u16::from(search) == raw
    }

    /// If no PDUs are present in the frame, set the first PDU index to the given value.
    unsafe fn set_first_pdu(this: NonNull<FrameElement<0>>, value: u8) {
        let first_pdu = &mut *addr_of_mut!((*this.as_ptr()).first_pdu);

        // Only set first PDU index if the frame is empty, as denoted by the `FIRST_PDU_EMPTY`
        // sentinel. Failures are ignored as we want a noop when the first PDU value was already
        // set.
        let _ = first_pdu.compare_exchange(
            FIRST_PDU_EMPTY,
            u16::from(value),
            Ordering::Release,
            Ordering::Relaxed,
        );
    }

    /// Clear first PDU.
    unsafe fn clear_first_pdu(this: NonNull<FrameElement<0>>) {
        let first_pdu = &*addr_of!((*this.as_ptr()).first_pdu);

        first_pdu.store(FIRST_PDU_EMPTY, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pdu_loop::frame_element::{AtomicFrameState, FrameElement, FIRST_PDU_EMPTY};
    use atomic_waker::AtomicWaker;
    use core::{ptr::NonNull, sync::atomic::AtomicU16};

    #[test]
    fn set_first_pdu_only_once() {
        let _ = env_logger::builder().is_test(true).try_init();

        const BUF_LEN: usize = 16;

        let frame = FrameElement {
            frame_index: 0xab,
            status: AtomicFrameState::new(FrameState::None),
            waker: AtomicWaker::default(),
            ethernet_frame: [0u8; BUF_LEN],
            pdu_payload_len: 0,
            first_pdu: AtomicU16::new(FIRST_PDU_EMPTY),
        };

        let frame_ptr = NonNull::from(&frame);

        unsafe { FrameElement::<0>::set_first_pdu(frame_ptr.cast(), 0xab) };
        unsafe { FrameElement::<0>::set_first_pdu(frame_ptr.cast(), 0xcd) };

        assert_eq!(frame.first_pdu.load(Ordering::Relaxed), 0xab);
    }

    #[test]
    fn find_empty_frame() {
        let _ = env_logger::builder().is_test(true).try_init();

        const BUF_LEN: usize = 16;

        let frame = FrameElement {
            frame_index: 0xab,
            status: AtomicFrameState::new(FrameState::None),
            waker: AtomicWaker::default(),
            ethernet_frame: [0u8; BUF_LEN],
            pdu_payload_len: 0,
            first_pdu: AtomicU16::new(FIRST_PDU_EMPTY),
        };

        let frame_ptr = NonNull::from(&frame);

        assert_eq!(
            unsafe { FrameElement::<0>::first_pdu_is(frame_ptr.cast(), 0) },
            false
        );
    }

    #[test]
    fn find_frame_zero() {
        let _ = env_logger::builder().is_test(true).try_init();

        const BUF_LEN: usize = 16;

        let frame = FrameElement {
            frame_index: 0xab,
            status: AtomicFrameState::new(FrameState::None),
            waker: AtomicWaker::default(),
            ethernet_frame: [0u8; BUF_LEN],
            pdu_payload_len: 0,
            first_pdu: AtomicU16::new(FIRST_PDU_EMPTY),
        };

        let frame_ptr = NonNull::from(&frame);

        unsafe { FrameElement::<0>::set_first_pdu(frame_ptr.cast(), 0) }

        assert_eq!(
            unsafe { FrameElement::<0>::first_pdu_is(frame_ptr.cast(), 0) },
            true
        );
    }

    #[test]
    fn find_frame_1() {
        let _ = env_logger::builder().is_test(true).try_init();

        const BUF_LEN: usize = 16;

        let frame_0 = FrameElement {
            frame_index: 0xab,
            status: AtomicFrameState::new(FrameState::None),
            waker: AtomicWaker::default(),
            ethernet_frame: [0u8; BUF_LEN],
            pdu_payload_len: 0,
            first_pdu: AtomicU16::new(FIRST_PDU_EMPTY),
        };

        let frame_ptr_0 = NonNull::from(&frame_0);

        unsafe { FrameElement::<0>::set_first_pdu(frame_ptr_0.cast(), 123) }

        // ---

        let frame_1 = FrameElement {
            frame_index: 0xab,
            status: AtomicFrameState::new(FrameState::None),
            waker: AtomicWaker::default(),
            ethernet_frame: [0u8; BUF_LEN],
            pdu_payload_len: 0,
            first_pdu: AtomicU16::new(FIRST_PDU_EMPTY),
        };

        let frame_ptr_1 = NonNull::from(&frame_1);

        unsafe { FrameElement::<0>::set_first_pdu(frame_ptr_1.cast(), 0xff) }

        // ---

        assert_eq!(
            unsafe { FrameElement::<0>::first_pdu_is(frame_ptr_0.cast(), 0) },
            false
        );
        assert_eq!(
            unsafe { FrameElement::<0>::first_pdu_is(frame_ptr_0.cast(), 123) },
            true
        );
        assert_eq!(
            unsafe { FrameElement::<0>::first_pdu_is(frame_ptr_0.cast(), 0xff) },
            false
        );

        assert_eq!(
            unsafe { FrameElement::<0>::first_pdu_is(frame_ptr_1.cast(), 0) },
            false
        );
        assert_eq!(
            unsafe { FrameElement::<0>::first_pdu_is(frame_ptr_1.cast(), 123) },
            false
        );
        assert_eq!(
            unsafe { FrameElement::<0>::first_pdu_is(frame_ptr_1.cast(), 0xff) },
            true
        );
    }
}
