use crate::{
    error::PduError,
    fmt,
    pdu_loop::{
        frame_element::{FrameElement, FrameState, PduMarker},
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
    frame: NonNull<FrameElement<0>>,
    pdu_idx: &'sto AtomicU8,
    max_len: usize,
    _lifetime: PhantomData<&'sto mut FrameElement<0>>,
}

impl<'sto> Debug for FrameBox<'sto> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let data = self.pdu_buf();

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
    pub fn new(
        frame: NonNull<FrameElement<0>>,
        pdu_idx: &'sto AtomicU8,
        max_len: usize,
    ) -> FrameBox<'sto> {
        Self {
            frame,
            max_len,
            pdu_idx,
            _lifetime: PhantomData,
        }
    }

    /// Reset Ethernet and EtherCAT headers, zero out Ethernet frame payload data.
    pub fn init(&mut self) {
        unsafe {
            addr_of_mut!((*self.frame.as_ptr()).waker).write(AtomicWaker::new());
            addr_of_mut!((*self.frame.as_ptr()).first_pdu).write(None);
        }

        let mut ethernet_frame = self.ethernet_frame_mut();

        ethernet_frame.set_src_addr(MASTER_ADDR);
        ethernet_frame.set_dst_addr(EthernetAddress::BROADCAST);
        ethernet_frame.set_ethertype(ETHERCAT_ETHERTYPE);
        ethernet_frame.payload_mut().fill(0);
    }

    pub fn next_pdu_idx(&self) -> u8 {
        self.pdu_idx.fetch_add(1, Ordering::Relaxed)
    }

    pub fn replace_waker(&self, waker: &Waker) {
        let ptr = unsafe { &*addr_of!((*self.frame.as_ptr()).waker) };

        ptr.register(waker);
    }

    pub fn wake(&self) -> Result<(), ()> {
        // SAFETY: `self.frame` is a `NonNull`, so `addr_of` will always point to valid data.
        let waker = unsafe { &*addr_of!((*self.frame.as_ptr()).waker) };

        if let Some(waker) = waker.take() {
            waker.wake();

            Ok(())
        } else {
            Err(())
        }
    }

    pub fn frame_index(&self) -> u8 {
        unsafe { FrameElement::<0>::frame_index(self.frame) }
    }

    /// Get EtherCAT frame header buffer.
    pub fn ecat_frame_header_mut(&mut self) -> &mut [u8] {
        let ptr = unsafe { FrameElement::<0>::ptr(self.frame) };

        let ethercat_header_start = EthernetFrame::<&[u8]>::header_len();

        unsafe {
            core::slice::from_raw_parts_mut(
                ptr.as_ptr().byte_add(ethercat_header_start),
                EthercatFrameHeader::PACKED_LEN,
            )
        }
    }

    /// Get frame payload for writing PDUs into
    pub fn pdu_buf_mut(&mut self) -> &mut [u8] {
        let ptr = unsafe { FrameElement::<0>::ethercat_payload_ptr(self.frame) };

        let pdu_payload_start =
            EthernetFrame::<&[u8]>::header_len() + EthercatFrameHeader::header_len();

        unsafe { core::slice::from_raw_parts_mut(ptr.as_ptr(), self.max_len - pdu_payload_start) }
    }

    /// Get frame payload area. This contains one or more PDUs and is located after the EtherCAT
    /// frame header.
    pub fn pdu_buf(&self) -> &[u8] {
        let ptr = unsafe { FrameElement::<0>::ethercat_payload_ptr(self.frame) };

        let pdu_payload_start =
            EthernetFrame::<&[u8]>::header_len() + EthercatFrameHeader::header_len();

        unsafe { core::slice::from_raw_parts(ptr.as_ptr(), self.max_len - pdu_payload_start) }
    }

    fn ethernet_frame_mut(&mut self) -> EthernetFrame<&mut [u8]> {
        // SAFETY: We hold a mutable reference to the containing `FrameBox`. A `FrameBox` can only
        // be created from a successful unique acquisition of a frame element.
        unsafe {
            EthernetFrame::new_unchecked(core::slice::from_raw_parts_mut(
                FrameElement::<0>::ptr(self.frame).as_ptr(),
                self.max_len,
            ))
        }
    }

    pub fn ethernet_frame(&self) -> EthernetFrame<&[u8]> {
        unsafe {
            EthernetFrame::new_unchecked(core::slice::from_raw_parts(
                FrameElement::<0>::ptr(self.frame).as_ptr(),
                self.max_len,
            ))
        }
    }

    pub fn pdu_payload_len(&self) -> usize {
        unsafe { *addr_of!((*self.frame.as_ptr()).pdu_payload_len) }
    }

    pub fn set_state(&self, to: FrameState) {
        unsafe { FrameElement::set_state(self.frame, to) };
    }

    pub fn swap_state(&self, from: FrameState, to: FrameState) -> Result<(), FrameState> {
        unsafe { FrameElement::swap_state(self.frame, from, to) }.map(|_| ())
    }

    pub fn add_pdu(&mut self, alloc_size: usize, pdu_idx: u8) {
        unsafe { *addr_of_mut!((*self.frame.as_ptr()).pdu_payload_len) += alloc_size };

        unsafe { FrameElement::<0>::set_first_pdu(self.frame, pdu_idx) };
    }
}
