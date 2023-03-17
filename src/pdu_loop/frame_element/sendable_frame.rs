use super::FrameBox;
use crate::{
    error::PduError,
    generate::{le_u16, le_u8, skip, write_packed, write_slice},
    pdu_loop::{
        frame_element::{FrameElement, FrameState},
        frame_header::FrameHeader,
    },
    ETHERCAT_ETHERTYPE, MASTER_ADDR,
};
use core::mem;
use smoltcp::wire::{EthernetAddress, EthernetFrame};

/// A frame has been initialised with valid header and data payload and is now ready to be picked up
/// by the TX loop and sent over the network.
#[derive(Debug)]
pub struct SendableFrame<'sto> {
    pub(in crate::pdu_loop) inner: FrameBox<'sto>,
}

impl<'a> SendableFrame<'a> {
    pub(in crate::pdu_loop) fn new(inner: FrameBox<'a>) -> Self {
        Self { inner }
    }

    /// The frame has been sent by the network driver.
    pub(in crate::pdu_loop) fn mark_sent(self) {
        log::trace!("Mark sent");

        unsafe {
            FrameElement::set_state(self.inner.frame, FrameState::Sending);
        }
    }

    /// The size of the total payload to be insterted into an EtherCAT frame.
    fn ethercat_payload_len(&self) -> u16 {
        // TODO: Add unit test to stop regressions
        let pdu_overhead = 12;

        unsafe { self.inner.frame() }.flags.len() + pdu_overhead
    }

    fn ethernet_payload_len(&self) -> usize {
        usize::from(self.ethercat_payload_len()) + mem::size_of::<FrameHeader>()
    }

    fn write_ethernet_payload<'buf>(&self, buf: &'buf mut [u8]) -> &'buf [u8] {
        let (frame, data) = unsafe { self.inner.frame_and_buf() };

        let header = FrameHeader::pdu(self.ethercat_payload_len());

        let buf = le_u16(header.0, buf);

        let buf = le_u8(frame.command.code() as u8, buf);
        let buf = le_u8(frame.index, buf);

        // Write address and register data
        let buf = write_slice(&frame.command.address(), buf);

        let buf = write_packed(frame.flags, buf);
        let buf = le_u16(frame.irq, buf);

        // Probably a read; the data area of the frame to send could be any old garbage, so we'll
        // skip over it.
        let buf = if data.is_empty() {
            skip(usize::from(frame.flags.len()), buf)
        }
        // Probably a write
        else {
            write_slice(data, buf)
        };

        // Working counter is always zero when sending
        let buf = le_u16(0u16, buf);

        buf
    }

    /// Write an Ethernet II frame containing the EtherCAT payload into `buf`.
    ///
    /// The consumed part of the buffer is returned on success, ready for passing to the network
    /// device. If the buffer is not large enough to hold the full frame, this method will return
    /// [`Error::Pdu(PduError::TooLong)`](PduError::TooLong).
    pub fn write_ethernet_packet<'buf>(&self, buf: &'buf mut [u8]) -> Result<&'buf [u8], PduError> {
        let ethernet_len = EthernetFrame::<&[u8]>::buffer_len(self.ethernet_payload_len());

        let buf = buf.get_mut(0..ethernet_len).ok_or(PduError::TooLong)?;

        let mut ethernet_frame = EthernetFrame::new_checked(buf).map_err(PduError::CreateFrame)?;

        ethernet_frame.set_src_addr(MASTER_ADDR);
        ethernet_frame.set_dst_addr(EthernetAddress::BROADCAST);
        ethernet_frame.set_ethertype(ETHERCAT_ETHERTYPE);

        let ethernet_payload = ethernet_frame.payload_mut();

        self.write_ethernet_payload(ethernet_payload);

        Ok(ethernet_frame.into_inner())
    }
}
