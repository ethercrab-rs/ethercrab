use super::pdu::PduFlags;
use crate::{
    command::Command,
    error::{Error, PduError},
    pdu_loop::{frame_header::FrameHeader, pdu::Pdu},
    ETHERCAT_ETHERTYPE, MASTER_ADDR,
};
use core::{
    future::Future,
    mem,
    pin::Pin,
    sync::atomic::Ordering,
    task::{Context, Poll, Waker},
};
use smoltcp::wire::{EthernetAddress, EthernetFrame};

#[atomic_enum::atomic_enum]
#[derive(PartialEq, Default)]
enum FrameState {
    // SAFETY: Because we create a bunch of `Frame`s with `MaybeUninit::zeroed`, the `None` state
    // MUST be equal to zero. All other fields in `Frame` are overridden in `replace`, so there
    // should be no UB there.
    #[default]
    None = 0x00,
    Created,
    Sending,
    Done,
}

impl Default for AtomicFrameState {
    fn default() -> Self {
        Self(Default::default())
    }
}

#[derive(Debug, Default)]
pub(crate) struct Frame {
    state: AtomicFrameState,
    waker: Option<Waker>,
    pub pdu: Pdu,
}

impl Frame {
    pub(crate) fn replace(
        &mut self,
        command: Command,
        data_length: u16,
        index: u8,
    ) -> Result<(), PduError> {
        let state = self.state.load(Ordering::SeqCst);

        if state != FrameState::None {
            trace!("Expected {:?}, got {:?}", FrameState::None, self.state);
            return Err(PduError::InvalidFrameState);
        }

        self.state.store(FrameState::Created, Ordering::SeqCst);

        let _ = self.waker.take();
        self.pdu.replace(command, data_length, index)?;

        Ok(())
    }

    pub(crate) fn pdu(&self) -> &Pdu {
        &self.pdu
    }

    /// The size of the total payload to be insterted into an Ethernet frame, i.e. EtherCAT frame
    /// payload and header.
    fn ethernet_payload_len(&self) -> usize {
        usize::from(self.pdu.ethercat_payload_len()) + mem::size_of::<FrameHeader>()
    }

    pub fn to_ethernet_frame<'a>(
        &self,
        buf: &'a mut [u8],
        data: &[u8],
    ) -> Result<&'a [u8], PduError> {
        let ethernet_len = EthernetFrame::<&[u8]>::buffer_len(self.ethernet_payload_len());

        let buf = buf.get_mut(0..ethernet_len).ok_or(PduError::TooLong)?;

        let mut ethernet_frame = EthernetFrame::new_checked(buf).map_err(PduError::CreateFrame)?;

        ethernet_frame.set_src_addr(MASTER_ADDR);
        ethernet_frame.set_dst_addr(EthernetAddress::BROADCAST);
        ethernet_frame.set_ethertype(ETHERCAT_ETHERTYPE);

        let ethernet_payload = ethernet_frame.payload_mut();

        self.pdu.to_ethernet_payload(ethernet_payload, data)?;

        Ok(ethernet_frame.into_inner())
    }

    pub(crate) fn wake_done(
        &mut self,
        flags: PduFlags,
        irq: u16,
        working_counter: u16,
    ) -> Result<(), PduError> {
        let state = self.state.load(Ordering::SeqCst);

        if state != FrameState::Sending {
            trace!("Expected {:?}, got {:?}", FrameState::Sending, self.state);
            return Err(PduError::InvalidFrameState);
        }

        let waker = self.waker.take().ok_or_else(|| {
            error!(
                "Attempted to wake frame #{} with no waker, possibly caused by timeout",
                self.pdu.index
            );

            PduError::InvalidFrameState
        })?;

        self.pdu.set_response(flags, irq, working_counter);

        self.state.store(FrameState::Done, Ordering::SeqCst);

        waker.wake();

        Ok(())
    }

    pub(crate) fn sendable(&mut self) -> Option<SendableFrame<'_>> {
        if self.state.load(Ordering::SeqCst) == FrameState::Created {
            Some(SendableFrame { frame: self })
        } else {
            None
        }
    }
}

/// A frame that is in a sendable state.
#[derive(Debug)]
pub struct SendableFrame<'a> {
    frame: &'a mut Frame,
}

impl<'a> SendableFrame<'a> {
    pub(crate) fn mark_sending(&mut self) {
        self.frame
            .state
            .store(FrameState::Sending, Ordering::SeqCst);
    }

    pub(crate) fn data_len(&self) -> usize {
        usize::from(self.frame.pdu.flags.len())
    }

    pub(crate) fn write_ethernet_packet<'buf>(
        &self,
        buf: &'buf mut [u8],
        data: &[u8],
    ) -> Result<&'buf [u8], PduError> {
        self.frame.to_ethernet_frame(buf, data)
    }
}

impl Future for Frame {
    type Output = Result<Pdu, Error>;

    fn poll(mut self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<Self::Output> {
        let state = self.state.load(Ordering::SeqCst);

        match state {
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
                self.state.store(FrameState::None, Ordering::SeqCst);

                // Drop waker so it doesn't get woken again
                self.waker.take();

                Poll::Ready(Ok(core::mem::take(&mut self.pdu)))
            }
        }
    }
}
