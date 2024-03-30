use crate::{
    error::{Error, PduError},
    fmt,
    pdu_loop::{
        frame_element::{FrameElement, PduMarker},
        frame_header::EthercatFrameHeader,
        PDU_SLOTS,
    },
    ETHERCAT_ETHERTYPE, MASTER_ADDR,
};
use atomic_waker::AtomicWaker;
use core::{
    alloc::Layout,
    fmt::Debug,
    marker::PhantomData,
    ptr::{addr_of, addr_of_mut, NonNull},
    sync::atomic::{AtomicU8, Ordering},
    task::Waker,
};
use ethercrab_wire::EtherCrabWireSized;
use smoltcp::wire::{EthernetAddress, EthernetFrame};

/// Frame data common to all typestates.
#[derive(Copy, Clone)]
pub struct FrameBox<'sto> {
    // NOTE: Only pub for tests
    pub(in crate::pdu_loop) frame: NonNull<FrameElement<0>>,
    _lifetime: PhantomData<&'sto mut FrameElement<0>>,
    pub(in crate::pdu_loop) pdu_markers: NonNull<PduMarker>,
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
            pdu_markers: pdu_states,
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
            pdu_markers: pdu_states,
            pdu_idx,
            _lifetime: PhantomData,
        })
    }

    pub fn next_pdu_idx(&self) -> u8 {
        self.pdu_idx.fetch_add(1, Ordering::Relaxed)
    }

    pub(in crate::pdu_loop) unsafe fn replace_waker(&self, waker: &Waker) {
        (*addr_of!((*self.frame.as_ptr()).waker)).register(waker);
    }

    pub(in crate::pdu_loop) unsafe fn wake(&self) -> Result<(), ()> {
        if let Some(waker) = (*addr_of!((*self.frame.as_ptr()).waker)).take() {
            waker.wake();

            Ok(())
        } else {
            Err(())
        }
    }

    pub(in crate::pdu_loop) fn frame_index(&self) -> u8 {
        FrameElement::<0>::frame_index(self.frame)
    }

    /// Get EtherCAT frame header buffer.
    pub(in crate::pdu_loop) unsafe fn ecat_frame_header_mut(&mut self) -> &mut [u8] {
        let ptr = FrameElement::<0>::ptr(self.frame);

        let ethercat_header_start = EthernetFrame::<&[u8]>::header_len();

        core::slice::from_raw_parts_mut(
            ptr.as_ptr().byte_add(ethercat_header_start),
            EthercatFrameHeader::PACKED_LEN,
        )
    }

    /// Get frame payload for writing PDUs into
    pub(in crate::pdu_loop) unsafe fn pdu_buf_mut(&mut self) -> &mut [u8] {
        let ptr = FrameElement::<0>::ethercat_payload_ptr(self.frame);

        let pdu_payload_start =
            EthernetFrame::<&[u8]>::header_len() + EthercatFrameHeader::header_len();

        core::slice::from_raw_parts_mut(ptr.as_ptr(), self.max_len - pdu_payload_start)
    }

    /// Get frame payload area.
    pub(in crate::pdu_loop) unsafe fn pdu_buf(&self) -> &[u8] {
        let ptr = FrameElement::<0>::ethercat_payload_ptr(self.frame);

        let pdu_payload_start =
            EthernetFrame::<&[u8]>::header_len() + EthercatFrameHeader::header_len();

        core::slice::from_raw_parts(ptr.as_ptr(), self.max_len - pdu_payload_start)
    }

    pub(in crate::pdu_loop) unsafe fn ethernet_frame(&self) -> EthernetFrame<&[u8]> {
        let ptr = FrameElement::<0>::ptr(self.frame);

        EthernetFrame::new_unchecked(core::slice::from_raw_parts(ptr.as_ptr(), self.max_len))
    }

    pub(crate) fn release_pdu_claims(&self) {
        let frame_index = u16::from(self.frame_index());

        fmt::trace!("Releasing PDUs from frame index {}", frame_index);

        let states: &[PduMarker] = unsafe {
            core::slice::from_raw_parts(self.pdu_markers.as_ptr() as *const _, PDU_SLOTS)
        };

        states
            .iter()
            .filter(|marker| marker.frame_index.load(Ordering::Relaxed) == frame_index)
            .take(usize::from(FrameElement::<0>::pdu_count(self.frame)))
            .for_each(|marker| {
                marker.release();
            });
    }

    pub(in crate::pdu_loop) fn pdu_payload_len(&self) -> usize {
        unsafe { *addr_of!((*self.frame.as_ptr()).pdu_payload_len) }
    }

    pub(in crate::pdu_loop) fn add_pdu_payload_len(&mut self, len: usize) {
        unsafe { *addr_of_mut!((*self.frame.as_ptr()).pdu_payload_len) += len };
    }

    pub(in crate::pdu_loop) fn reserve_pdu_marker(&self, frame_index: u8) -> Result<u8, PduError> {
        let pdu_idx = self.next_pdu_idx();

        // Sanity check. PDU_SLOTS is currently 256 which is fine, but if that changes, this assert
        // should catch the logic bug.
        assert!(usize::from(pdu_idx) < PDU_SLOTS);

        let marker = unsafe {
            let base_ptr = self.pdu_markers.as_ptr() as *const PduMarker;

            let layout = Layout::array::<PduMarker>(PDU_SLOTS).unwrap();

            let stride = layout.size() / PDU_SLOTS;

            let this_marker = base_ptr.byte_add(usize::from(pdu_idx) * stride);

            &*this_marker
        };

        marker.reserve(frame_index)?;

        Ok(pdu_idx)
    }
}
