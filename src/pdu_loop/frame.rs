//! An EtherCAT frame with various payloads.

use crate::error::{Error, PduError, PduValidationError};
use crate::pdu_loop::frame_header::FrameHeader;
use crate::pdu_loop::pdu::Pdu;
use crate::{ETHERCAT_ETHERTYPE, MASTER_ADDR};
use cookie_factory::{gen_simple, GenError};
use core::future::Future;
use core::pin::Pin;
use core::task::{Context, Poll, Waker};
use smoltcp::wire::{EthernetAddress, EthernetFrame};

#[derive(Debug, PartialEq, Default)]
pub(crate) enum FrameState {
    #[default]
    None,
    Created,
    Sending,
    Done,
}

pub trait FramePayload {
    fn as_bytes<'buf>(&self, buf: &'buf mut [u8]) -> Result<&'buf [u8], GenError>;
    fn len(&self) -> usize;
    fn is_response_to(&self, request_pdu: &Self) -> Result<(), PduValidationError>;
    fn index(&self) -> u8;
}

#[derive(Debug, Default)]
pub(crate) struct Frame<T> {
    state: FrameState,
    waker: Option<Waker>,
    data: T,
}

impl<T> Frame<T>
where
    T: FramePayload,
{
    pub(crate) fn replace(&mut self, data: T) -> Result<(), PduError> {
        if self.state != FrameState::None {
            trace!("Expected {:?}, got {:?}", FrameState::None, self.state);
            Err(PduError::InvalidFrameState)?;
        }

        *self = Self {
            state: FrameState::Created,
            waker: None,
            data,
        };

        Ok(())
    }

    pub(crate) fn sendable(&mut self) -> Option<SendableFrame<'_, T>> {
        if self.state == FrameState::Created {
            Some(SendableFrame { frame: self })
        } else {
            None
        }
    }

    pub(crate) fn wake_done(&mut self, data: T) -> Result<(), PduError> {
        if self.state != FrameState::Sending {
            trace!("Expected {:?}, got {:?}", FrameState::Sending, self.state);
            Err(PduError::InvalidFrameState)?;
        }

        let waker = self.waker.take().ok_or_else(|| {
            error!(
                "Attempted to wake frame #{} with no waker, possibly caused by timeout",
                data.index()
            );

            PduError::InvalidFrameState
        })?;

        data.is_response_to(&self.data)?;

        self.data = data;
        self.state = FrameState::Done;

        waker.wake();

        Ok(())
    }
}

/// A frame that is in a sendable state.
pub struct SendableFrame<'a, T> {
    frame: &'a mut Frame<T>,
}

impl<'a, T> SendableFrame<'a, T>
where
    T: FramePayload,
{
    pub(crate) fn mark_sending(&mut self) {
        self.frame.state = FrameState::Sending;
    }

    /// Take a mutable buffer and write a complete Ethernet packet into it, containing an EtherCAT
    /// packet and payload.
    pub fn write_ethernet_frame<'buf>(&self, buf: &'buf mut [u8]) -> Result<&'buf [u8], PduError> {
        let data_and_header = self.frame.data.len() + 2;

        let ethernet_len = EthernetFrame::<&[u8]>::buffer_len(data_and_header);

        let buf = buf.get_mut(0..ethernet_len).ok_or(PduError::TooLong)?;

        let mut ethernet_frame = EthernetFrame::new_checked(buf).map_err(PduError::CreateFrame)?;

        ethernet_frame.set_src_addr(MASTER_ADDR);
        ethernet_frame.set_dst_addr(EthernetAddress::BROADCAST);
        ethernet_frame.set_ethertype(ETHERCAT_ETHERTYPE);

        let header = FrameHeader::pdu(self.frame.data.len());

        let buf = ethernet_frame.payload_mut();

        let buf =
            gen_simple(cookie_factory::bytes::le_u16(header.0), buf).map_err(PduError::Encode)?;
        let _buf = self.frame.data.as_bytes(buf).map_err(PduError::Encode)?;

        let buf = ethernet_frame.into_inner();

        Ok(buf)
    }
}

// NOTE: Using a macro to sidestep weird pinning issues with bounds around `T` when working with
// `Frame<T>`.
macro_rules! impl_fut {
    ($payload:ty) => {
        impl<const MAX_PDU_DATA: usize> Future for Frame<$payload> {
            type Output = Result<$payload, Error>;

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

                        Poll::Ready(Ok(core::mem::take(&mut self.data)))
                    }
                }
            }
        }
    };
}

impl_fut!(Pdu<MAX_PDU_DATA>);
