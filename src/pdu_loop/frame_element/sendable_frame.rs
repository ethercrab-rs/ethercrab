use super::FrameBox;
use crate::{
    error::{Error, PduError},
    generate::{le_u16, le_u8, skip, write_packed, write_slice},
    pdu_loop::{
        frame_element::{FrameElement, FrameState},
        frame_header::FrameHeader,
    },
    ETHERCAT_ETHERTYPE, MASTER_ADDR,
};
use core::future::Future;
use core::mem;
use smoltcp::wire::{EthernetAddress, EthernetFrame};

/// An EtherCAT frame that is ready to be sent over the network.
///
/// This struct can be acquired by calling
/// [`PduLoop::next_sendable_frame`](crate::pdu_loop::PduTx::next_sendable_frame).
///
/// # Examples
///
/// ```rust,no_run
/// # use ethercrab::PduStorage;
/// use core::future::poll_fn;
/// use core::task::Poll;
///
/// # static PDU_STORAGE: PduStorage<2, 2> = PduStorage::new();
/// let (mut pdu_tx, _pdu_rx, _pdu_loop) = PDU_STORAGE.try_split().expect("can only split once");
///
/// poll_fn(|ctx| {
///     if let Some(packet) = pdu_tx.next_sendable_frame() {
///         // Send packet over the network interface here
///
///         // Wake the future so it's polled again in case there are more frames to send
///         ctx.waker().wake_by_ref();
///     }
///
///     // Set the waker so this future is polled again when new EtherCAT frames are ready to
///     // be sent.
///     pdu_tx.waker().replace(ctx.waker().clone());
///
///     Poll::<()>::Pending
/// });
/// ```
#[derive(Debug)]
pub struct SendableFrame<'sto> {
    pub(in crate::pdu_loop) inner: FrameBox<'sto>,
}

unsafe impl<'sto> Send for SendableFrame<'sto> {}

impl<'sto> SendableFrame<'sto> {
    pub(in crate::pdu_loop) fn new(inner: FrameBox<'sto>) -> Self {
        Self { inner }
    }

    /// The frame has been sent by the network driver.
    pub(crate) fn mark_sent(self) {
        unsafe {
            FrameElement::set_state(self.inner.frame, FrameState::Sending);
        }
    }

    /// The size of the total payload to be insterted into an EtherCAT frame.
    fn ethercat_payload_len(&self) -> u16 {
        let pdu_overhead = 12;

        unsafe { self.inner.frame() }.flags.len() + pdu_overhead
    }

    /// The length in bytes required to hold the full Ethernet II frame, containing an EtherCAT
    /// payload.
    // Clippy: We don't care if the frame is empty or not
    #[allow(clippy::len_without_is_empty)]
    pub fn len(&self) -> usize {
        EthernetFrame::<&[u8]>::buffer_len(self.ethernet_payload_len())
    }

    /// The length in bytes required to hold a full EtherCAT frame.
    fn ethernet_payload_len(&self) -> usize {
        usize::from(self.ethercat_payload_len()) + mem::size_of::<FrameHeader>()
    }

    fn write_ethernet_payload<'buf>(&self, buf: &'buf mut [u8]) -> &'buf [u8] {
        let (frame, data) = unsafe { self.inner.frame_and_buf() };

        let header = FrameHeader::pdu(self.ethercat_payload_len());

        let buf = le_u16(header.0, buf);

        let buf = le_u8(frame.command.code(), buf);
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
        let ethernet_len = self.len();

        let buf = buf.get_mut(0..ethernet_len).ok_or(PduError::TooLong)?;

        let mut ethernet_frame = EthernetFrame::new_checked(buf).map_err(PduError::CreateFrame)?;

        ethernet_frame.set_src_addr(MASTER_ADDR);
        ethernet_frame.set_dst_addr(EthernetAddress::BROADCAST);
        ethernet_frame.set_ethertype(ETHERCAT_ETHERTYPE);

        let ethernet_payload = ethernet_frame.payload_mut();

        self.write_ethernet_payload(ethernet_payload);

        Ok(ethernet_frame.into_inner())
    }

    /// Send the frame using a callback returning a future.
    pub async fn send<'buf, F, O>(self, packet_buf: &'buf mut [u8], send: F) -> Result<(), Error>
    where
        F: FnOnce(&'buf [u8]) -> O,
        O: Future<Output = Result<(), Error>>,
    {
        let bytes = self.write_ethernet_packet(packet_buf)?;

        send(bytes).await?;

        // FIXME: Release frame on failure

        self.mark_sent();

        Ok(())
    }

    /// Send the frame using a blocking callback.
    pub fn send_blocking<'buf>(
        self,
        packet_buf: &'buf mut [u8],
        send: impl FnOnce(&'buf [u8]) -> Result<(), Error>,
    ) -> Result<(), Error> {
        let bytes = self.write_ethernet_packet(packet_buf)?;

        send(bytes)?;

        // FIXME: Release frame on failure

        self.mark_sent();

        Ok(())
    }
}
