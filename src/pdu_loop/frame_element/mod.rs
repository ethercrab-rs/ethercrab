use super::{frame_header::EthercatFrameHeader, PDU_UNUSED_SENTINEL};
use crate::{
    command::Command,
    error::{Error, PduError},
    fmt,
    pdu_loop::{pdu_flags::PduFlags, PDU_SLOTS},
    ETHERCAT_ETHERTYPE, MASTER_ADDR,
};
use atomic_waker::AtomicWaker;
use core::{
    alloc::Layout,
    cell::Cell,
    fmt::Debug,
    marker::PhantomData,
    ptr::{addr_of, addr_of_mut, NonNull},
    sync::atomic::{AtomicU16, AtomicU8, Ordering},
    task::Waker,
};
use ethercrab_wire::EtherCrabWireSized;
use smoltcp::wire::{EthernetAddress, EthernetFrame};

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

#[derive(Debug)]
pub struct PduMarker {
    /// Ethernet frame index (`u8`) plus marker to indicate if this PDU is in flight (uses `u16`
    /// high bits).
    ///
    /// The marker value is defined by [`PDU_UNUSED_SENTINEL`].
    frame_index: AtomicU16,

    // NOTE: Not keeping PDU index as this is implicit to the PDU -> frame mapping array. No sense
    // in duplicating information.
    inner: Cell<PduMarkerInner>,
}

#[derive(Copy, Clone, Debug, Default)]
pub struct PduMarkerInner {
    // Keep so we can check received PDU against this one
    pub command: Command,
    // Keep so we can check received PDU against this one
    pub flags: PduFlags,
    // Always sent as zero plus we never check or use it currently
    // pub irq: u16,
    // Sent working counter is always zero, and the received WKC is checked outside the PDU loop
    // pub working_counter: u16,
}

impl PduMarker {
    /// Try to reserve this PDU for use in a TX/RX.
    ///
    /// If the given index is already reserved, an error will be returned.
    pub fn reserve(
        &self,
        frame_idx: u8,
        command: Command,
        flags: PduFlags,
    ) -> Result<(), PduError> {
        // Try to reserve the frame by switching the flag state from unused to the frame
        if let Err(bad_state) = self.frame_index.compare_exchange(
            PDU_UNUSED_SENTINEL,
            u16::from(frame_idx),
            Ordering::Acquire,
            Ordering::Relaxed,
        ) {
            fmt::error!(
                "Bad PDU marker state: points to existing frame index {}, expecting sentinel {}, new frame index {}",
                bad_state,
                PDU_UNUSED_SENTINEL,
                frame_idx
            );

            // TODO: Maybe a unique error variant here?
            return Err(PduError::InvalidFrameState);
        }

        self.inner.replace(PduMarkerInner { command, flags });

        Ok(())
    }

    pub fn frame_index(&self) -> u8 {
        let raw = self.frame_index.load(Ordering::Relaxed);

        assert_ne!(raw, PDU_UNUSED_SENTINEL);

        raw as u8
    }

    pub fn init(&self) {
        assert!(self
            .frame_index
            .compare_exchange(0, PDU_UNUSED_SENTINEL, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok());
    }

    /// Reset this marker to unused if it belongs to the given frame index.
    fn release_for_frame(&self, frame_index: u16) -> Result<(), ()> {
        // This is much more performant than `compare_exchange`, even though it's a bit messier :(
        if self.frame_index.load(Ordering::Relaxed) == frame_index {
            self.frame_index
                .store(PDU_UNUSED_SENTINEL, Ordering::Release);

            Ok(())
        } else {
            Err(())
        }
    }
}

/// An individual frame state, PDU header config, and data buffer.
///
/// # A frame's journey
///
/// TODO: Update this journey! The current docs are out of date!
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
    pdu_payload_len: usize,
    /// The number of PDU handles held by this frame.
    ///
    /// Used to drop the whole frame only when all PDUs have been consumed from it.
    refcount: u8,

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
            refcount: 0,
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
        (*addr_of_mut!((*this.as_ptr()).refcount)) = 0;

        Ok(this)
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
                    "Failed to claim receiving frame {}: expected state {:?}, but got {:?}",
                    (*addr_of_mut!((*this.as_ptr()).frame_index)),
                    FrameState::Sent,
                    actual_state
                );
            })
            .ok()
    }

    fn inc_refcount(this: NonNull<FrameElement<0>>) -> u8 {
        let count = unsafe { &mut *addr_of_mut!((*this.as_ptr()).refcount) };

        let new = count.saturating_add(1);

        *count = new;

        new
    }

    fn dec_refcount(this: NonNull<FrameElement<0>>) -> u8 {
        let count = unsafe { &mut *addr_of_mut!((*this.as_ptr()).refcount) };

        let new = count.saturating_sub(1);

        *count = new;

        new
    }

    fn frame_index(this: NonNull<FrameElement<0>>) -> u8 {
        unsafe { *addr_of!((*this.as_ptr()).frame_index) }
    }
}

/// Frame data common to all typestates.
#[derive(Copy, Clone)]
pub struct FrameBox<'sto> {
    // NOTE: Only pub for tests
    pub(in crate::pdu_loop) frame: NonNull<FrameElement<0>>,
    _lifetime: PhantomData<&'sto mut FrameElement<0>>,
    pdu_states: NonNull<PduMarker>,
    pdu_idx: &'sto AtomicU8,
    max_len: usize,
}

impl<'sto> Debug for FrameBox<'sto> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let data = unsafe { self.pdu_buf() };

        f.debug_struct("FrameBox")
            .field("state", unsafe {
                &(*addr_of!((*self.frame.as_ptr()).status))
            })
            .field("frame_index", &self.frame_index())
            .field("data_hex", &format_args!("{:02x?}", data))
            .finish()
    }
}

impl<'sto> FrameBox<'sto> {
    /// Wrap a [`FrameElement`] pointer in a `FrameBox` without modifying the underlying data.
    pub(crate) fn new(
        frame: NonNull<FrameElement<0>>,
        pdu_states: NonNull<PduMarker>,
        pdu_idx: &'sto AtomicU8,
        max_len: usize,
    ) -> FrameBox<'sto> {
        Self {
            frame,
            max_len,
            pdu_states,
            pdu_idx,
            _lifetime: PhantomData,
        }
    }

    /// Wrap a [`FrameElement`] pointer in a `FrameBox` but reset Ethernet and EtherCAT headers, as
    /// well as zero out data payload.
    pub(crate) fn init(
        frame: NonNull<FrameElement<0>>,
        pdu_states: NonNull<PduMarker>,

        pdu_idx: &'sto AtomicU8,
        max_len: usize,
    ) -> Result<FrameBox<'sto>, Error> {
        unsafe {
            addr_of_mut!((*frame.as_ptr()).waker).write(AtomicWaker::new());
        }

        let mut ethernet_frame = unsafe {
            let buf = core::slice::from_raw_parts_mut(
                addr_of_mut!((*frame.as_ptr()).ethernet_frame).cast(),
                max_len,
            );

            EthernetFrame::new_checked(buf)?
        };

        ethernet_frame.set_src_addr(MASTER_ADDR);
        ethernet_frame.set_dst_addr(EthernetAddress::BROADCAST);
        ethernet_frame.set_ethertype(ETHERCAT_ETHERTYPE);

        ethernet_frame.payload_mut().fill(0);

        Ok(Self {
            max_len,
            frame,
            pdu_states,
            pdu_idx,
            _lifetime: PhantomData,
        })
    }

    pub fn next_pdu_idx(&self) -> u8 {
        self.pdu_idx.fetch_add(1, Ordering::Relaxed)
    }

    unsafe fn replace_waker(&self, waker: &Waker) {
        (*addr_of!((*self.frame.as_ptr()).waker)).register(waker);
    }

    unsafe fn wake(&self) -> Result<(), ()> {
        if let Some(waker) = (*addr_of!((*self.frame.as_ptr()).waker)).take() {
            waker.wake();

            Ok(())
        } else {
            Err(())
        }
    }

    fn frame_index(&self) -> u8 {
        FrameElement::<0>::frame_index(self.frame)
    }

    /// Get EtherCAT frame header buffer.
    unsafe fn ecat_frame_header_mut(&mut self) -> &mut [u8] {
        let ptr = FrameElement::<0>::ptr(self.frame);

        let ethercat_header_start = EthernetFrame::<&[u8]>::header_len();

        core::slice::from_raw_parts_mut(
            ptr.as_ptr().byte_add(ethercat_header_start),
            EthercatFrameHeader::PACKED_LEN,
        )
    }

    /// Get frame payload for writing PDUs into
    unsafe fn pdu_buf_mut(&mut self) -> &mut [u8] {
        let ptr = FrameElement::<0>::ethercat_payload_ptr(self.frame);

        let pdu_payload_start =
            EthernetFrame::<&[u8]>::header_len() + EthercatFrameHeader::header_len();

        core::slice::from_raw_parts_mut(ptr.as_ptr(), self.max_len - pdu_payload_start)
    }

    /// Get frame payload area.
    unsafe fn pdu_buf(&self) -> &[u8] {
        let ptr = FrameElement::<0>::ethercat_payload_ptr(self.frame);

        let pdu_payload_start =
            EthernetFrame::<&[u8]>::header_len() + EthercatFrameHeader::header_len();

        core::slice::from_raw_parts(ptr.as_ptr(), self.max_len - pdu_payload_start)
    }

    unsafe fn ethernet_frame(&self) -> EthernetFrame<&[u8]> {
        let ptr = FrameElement::<0>::ptr(self.frame);

        EthernetFrame::new_unchecked(core::slice::from_raw_parts(ptr.as_ptr(), self.max_len))
    }

    pub(crate) fn release_pdu_claims(&self) {
        let frame_index = u16::from(self.frame_index());

        fmt::trace!("Releasing PDUs from frame index {}", frame_index);

        let states: &[PduMarker] =
            unsafe { core::slice::from_raw_parts(self.pdu_states.as_ptr() as *const _, PDU_SLOTS) };

        for state in states {
            state.release_for_frame(frame_index).ok();
        }
    }

    pub(in crate::pdu_loop) fn pdu_payload_len(&self) -> usize {
        unsafe { *addr_of!((*self.frame.as_ptr()).pdu_payload_len) }
    }

    pub(in crate::pdu_loop) fn add_pdu_payload_len(&mut self, len: usize) {
        unsafe { *addr_of_mut!((*self.frame.as_ptr()).pdu_payload_len) += len };
    }

    fn reserve_pdu_marker(
        &self,
        frame_index: u8,
        command: Command,
        flags: PduFlags,
    ) -> Result<u8, PduError> {
        let pdu_idx = self.next_pdu_idx();

        // Sanity check. PDU_SLOTS is currently 256 which is fine, but if that changes, this assert
        // should catch the logic bug.
        assert!(usize::from(pdu_idx) < PDU_SLOTS);

        let marker = unsafe {
            let base_ptr = self.pdu_states.as_ptr() as *const PduMarker;

            let layout = Layout::array::<PduMarker>(PDU_SLOTS).unwrap();

            let stride = layout.size() / PDU_SLOTS;

            let this_marker = base_ptr.byte_add(usize::from(pdu_idx) * stride);

            &*this_marker
        };

        marker.reserve(frame_index, command, flags)?;

        Ok(pdu_idx)
    }
}
