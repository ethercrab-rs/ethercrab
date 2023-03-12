use super::FrameBox;
use crate::{
    error::PduError,
    pdu_loop::{
        frame_element::{FrameElement, FrameState},
        frame_header::FrameHeader,
    },
    ETHERCAT_ETHERTYPE, MASTER_ADDR,
};
use cookie_factory::{
    bytes::{le_u16, le_u8},
    combinator::{skip, slice},
    gen_simple, GenError,
};
use core::mem;
use packed_struct::PackedStruct;
use smoltcp::wire::{EthernetAddress, EthernetFrame};

/// A frame has been initialised with valid header and data payload and is now ready to be picked up
/// by the TX loop and sent over the network.
#[derive(Debug)]
pub struct SendableFrame<'sto> {
    pub(in crate::pdu_loop) inner: FrameBox<'sto>,
}

impl<'a> SendableFrame<'a> {
    pub fn new(inner: FrameBox<'a>) -> Self {
        Self { inner }
    }

    /// The frame has been sent by the network driver.
    pub fn mark_sent(self) {
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

    fn write_ethernet_payload<'buf>(&self, buf: &'buf mut [u8]) -> Result<&'buf [u8], PduError> {
        let (frame, data) = unsafe { self.inner.frame_and_buf() };

        let header = FrameHeader::pdu(self.ethercat_payload_len());

        let buf = gen_simple(le_u16(header.0), buf).map_err(PduError::Encode)?;

        let buf = gen_simple(le_u8(frame.command.code() as u8), buf)?;
        let buf = gen_simple(le_u8(frame.index), buf)?;

        // Write address and register data
        let buf = gen_simple(slice(frame.command.address()?), buf)?;

        let buf = gen_simple(le_u16(u16::from_le_bytes(frame.flags.pack().unwrap())), buf)?;
        let buf = gen_simple(le_u16(frame.irq), buf)?;

        // Probably a read; the data area of the frame to send could be any old garbage, so we'll
        // skip over it.
        let buf = if data.is_empty() {
            gen_simple(skip(usize::from(frame.flags.len())), buf)?
        }
        // Probably a write
        else {
            gen_simple(slice(data), buf)?
        };

        // Working counter is always zero when sending
        let buf = gen_simple(le_u16(0u16), buf)?;

        if !buf.is_empty() {
            log::error!(
                "Expected fully used buffer, got {} bytes left instead",
                buf.len()
            );

            Err(PduError::Encode(GenError::BufferTooBig(buf.len())))
        } else {
            Ok(buf)
        }
    }

    pub fn write_ethernet_packet<'buf>(&self, buf: &'buf mut [u8]) -> Result<&'buf [u8], PduError> {
        let ethernet_len = EthernetFrame::<&[u8]>::buffer_len(self.ethernet_payload_len());

        let buf = buf.get_mut(0..ethernet_len).ok_or(PduError::TooLong)?;

        let mut ethernet_frame = EthernetFrame::new_checked(buf).map_err(PduError::CreateFrame)?;

        ethernet_frame.set_src_addr(MASTER_ADDR);
        ethernet_frame.set_dst_addr(EthernetAddress::BROADCAST);
        ethernet_frame.set_ethertype(ETHERCAT_ETHERTYPE);

        let ethernet_payload = ethernet_frame.payload_mut();

        self.write_ethernet_payload(ethernet_payload)?;

        Ok(ethernet_frame.into_inner())
    }
}
