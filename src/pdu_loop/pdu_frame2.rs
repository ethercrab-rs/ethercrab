use super::pdu::PduFlags;
use super::pdu2::{self, Pdu2};
use crate::command::Command;
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
pub(crate) struct Frame2<'a> {
    state: FrameState,
    waker: Option<Waker>,
    // data: &'a [u8],
    pdu: Pdu2<'a>,
}

impl<'a> Frame2<'a> {
    pub(crate) fn replace(
        &mut self,
        command: Command,
        data_length: u16,
        index: u8,
        data: &'a [u8],
    ) -> Result<(), PduError> {
        if self.state != FrameState::None {
            trace!("Expected {:?}, got {:?}", FrameState::None, self.state);
            Err(PduError::InvalidFrameState)?;
        }

        self.state = FrameState::Created;
        self.waker = None;
        self.pdu.replace(command, data_length, index, data)?;

        Ok(())
    }

    pub(crate) fn pdu(&self) -> &Pdu2<'a> {
        &self.pdu
    }

    /// The size of the total payload to be insterted into an Ethernet frame, i.e. EtherCAT frame
    /// payload and header.
    fn ethernet_payload_len(&self) -> usize {
        self.pdu.ethercat_payload_len() + mem::size_of::<FrameHeader>()
    }

    pub fn ethernet_frame_len(&self) -> usize {
        EthernetFrame::<&[u8]>::buffer_len(self.ethernet_payload_len())
    }

    pub fn to_ethernet_frame<'buf>(&self, buf: &'buf mut [u8]) -> Result<&'buf [u8], PduError> {
        let ethernet_len = self.ethernet_frame_len();

        let buf = buf.get_mut(0..ethernet_len).ok_or(PduError::TooLong)?;

        let mut ethernet_frame = EthernetFrame::new_checked(buf).map_err(PduError::CreateFrame)?;

        ethernet_frame.set_src_addr(MASTER_ADDR);
        ethernet_frame.set_dst_addr(EthernetAddress::BROADCAST);
        ethernet_frame.set_ethertype(ETHERCAT_ETHERTYPE);

        let ethernet_payload = ethernet_frame.payload_mut();

        self.pdu.to_ethernet_payload(ethernet_payload)?;

        Ok(ethernet_frame.into_inner())
    }

    pub(crate) fn wake_done(
        &mut self,
        flags: pdu2::PduFlags,
        irq: u16,
        data: &'a [u8],
        working_counter: u16,
    ) -> Result<(), PduError> {
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

        self.pdu.set_response(flags, irq, data, working_counter)?;

        self.state = FrameState::Done;

        waker.wake();

        Ok(())
    }

    pub(crate) fn sendable(&'a mut self) -> Option<SendableFrame<'a>> {
        if self.state == FrameState::Created {
            Some(SendableFrame { frame: self })
        } else {
            None
        }
    }
}

/// A frame that is in a sendable state.
pub struct SendableFrame<'a> {
    frame: &'a mut Frame2<'a>,
}

impl<'a> SendableFrame<'a> {
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

impl<'a> Future for Frame2<'a> {
    type Output = Result<u16, Error>;

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

                Poll::Ready(Ok(self.pdu.working_counter()))
            }
        }
    }
}
