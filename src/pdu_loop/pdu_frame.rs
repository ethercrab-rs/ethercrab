use crate::error::{Error, PduError};
use crate::pdu_loop::pdu::Pdu;
use crate::{pdu_loop::frame_header::FrameHeader, ETHERCAT_ETHERTYPE, MASTER_ADDR};
use core::future::Future;
use core::mem;
use core::pin::Pin;
use core::task::{Context, Poll, Waker};
use smoltcp::wire::{EthernetAddress, EthernetFrame};

#[derive(Debug, PartialEq, Default)]
pub(crate) enum FrameState {
    // SAFETY: Because we create a bunch of `Frame`s with `MaybeUninit::zeroed`, the `None` state
    // MUST be equal to zero. All other fields in `Frame` are overridden in `replace`, so there
    // should be no UB there.
    #[default]
    None = 0x00,
    Created,
    Sending,
    Done,
}

#[derive(Debug, Default)]
pub(crate) struct Frame<const MAX_PDU_DATA: usize> {
    state: FrameState,
    waker: Option<Waker>,
    pdu: Pdu<MAX_PDU_DATA>,
}

impl<const MAX_PDU_DATA: usize> Frame<MAX_PDU_DATA> {
    pub(crate) fn replace(&mut self, pdu: Pdu<MAX_PDU_DATA>) -> Result<(), PduError> {
        if self.state != FrameState::None {
            trace!("Expected {:?}, got {:?}", FrameState::None, self.state);
            Err(PduError::InvalidFrameState)?;
        }

        *self = Self {
            state: FrameState::Created,
            waker: None,
            pdu,
        };

        Ok(())
    }

    pub(crate) fn pdu(&mut self) -> &mut Pdu<MAX_PDU_DATA> {
        &mut self.pdu
    }

    /// The size of the total payload to be insterted into an Ethernet frame, i.e. EtherCAT frame
    /// payload and header.
    fn ethernet_payload_len(&self) -> usize {
        self.pdu.ethercat_payload_len() + mem::size_of::<FrameHeader>()
    }

    pub fn to_ethernet_frame<'a>(&self, buf: &'a mut [u8]) -> Result<&'a [u8], PduError> {
        let ethernet_len = EthernetFrame::<&[u8]>::buffer_len(self.ethernet_payload_len());

        let buf = buf.get_mut(0..ethernet_len).ok_or(PduError::TooLong)?;

        let mut ethernet_frame = EthernetFrame::new_checked(buf).map_err(PduError::CreateFrame)?;

        ethernet_frame.set_src_addr(MASTER_ADDR);
        ethernet_frame.set_dst_addr(EthernetAddress::BROADCAST);
        ethernet_frame.set_ethertype(ETHERCAT_ETHERTYPE);

        let ethernet_payload = ethernet_frame.payload_mut();

        self.pdu.to_ethernet_payload(ethernet_payload)?;

        Ok(ethernet_frame.into_inner())
    }

    pub(crate) fn wake_done(&mut self) -> Result<(), PduError> {
        if self.state != FrameState::Sending {
            trace!("Expected {:?}, got {:?}", FrameState::Sending, self.state);
            Err(PduError::InvalidFrameState)?;
        }

        let waker = self.waker.take().ok_or_else(|| {
            error!(
                "Attempted to wake frame #{} with no waker, possibly caused by timeout",
                self.pdu.index()
            );

            PduError::InvalidFrameState
        })?;

        self.state = FrameState::Done;

        waker.wake();

        Ok(())
    }

    pub(crate) fn sendable(&mut self) -> Option<SendableFrame<'_, MAX_PDU_DATA>> {
        if self.state == FrameState::Created {
            Some(SendableFrame { frame: self })
        } else {
            None
        }
    }
}

/// A frame that is in a sendable state.
pub struct SendableFrame<'a, const MAX_PDU_DATA: usize> {
    frame: &'a mut Frame<MAX_PDU_DATA>,
}

impl<'a, const MAX_PDU_DATA: usize> SendableFrame<'a, MAX_PDU_DATA> {
    pub(crate) fn mark_sending(&mut self) {
        self.frame.state = FrameState::Sending;
    }

    pub(crate) fn write_ethernet_packet<'buf>(
        &self,
        buf: &'buf mut [u8],
    ) -> Result<&'buf [u8], PduError> {
        self.frame.to_ethernet_frame(buf)
    }
}

impl<const MAX_PDU_DATA: usize> Future for Frame<MAX_PDU_DATA> {
    type Output = Result<Pdu<MAX_PDU_DATA>, Error>;

    fn poll(mut self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.state {
            FrameState::None => {
                trace!("Frame future polled in None state");
                Poll::Ready(Err(Error::Pdu(PduError::InvalidFrameState)))
            }
            FrameState::Created | FrameState::Sending => {
                // NOTE: Drops previous waker
                self.waker.replace(ctx.waker().clone());

                Poll::Pending
            }
            FrameState::Done => {
                // Clear frame state ready for reuse
                self.state = FrameState::None;

                // Drop waker so it doesn't get woken again
                self.waker.take();

                Poll::Ready(Ok(core::mem::take(&mut self.pdu)))
            }
        }
    }
}
