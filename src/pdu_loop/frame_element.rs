use crate::{
    command::Command,
    error::{Error, PduError},
    pdu_data::PduRead,
    pdu_loop::{frame_header::FrameHeader, pdu_flags::PduFlags},
    ETHERCAT_ETHERTYPE, MASTER_ADDR,
};
use cookie_factory::{
    bytes::{le_u16, le_u8},
    combinator::{skip, slice},
    gen_simple, GenError,
};
use core::{
    any::type_name,
    future::Future,
    marker::PhantomData,
    mem,
    ptr::{addr_of, addr_of_mut, NonNull},
    sync::atomic::Ordering,
    task::Poll,
};
use packed_struct::PackedStruct;
use smoltcp::wire::{EthernetAddress, EthernetFrame};
use std::task::Waker;

#[atomic_enum::atomic_enum]
#[derive(PartialEq, Default)]
pub enum FrameState {
    // SAFETY: Because we create a bunch of `Frame`s with `MaybeUninit::zeroed`, the `None` state
    // MUST be equal to zero. All other fields in `Frame` are overridden in `replace`, so there
    // should be no UB there.
    /// The frame is ready to be claimed
    #[default]
    None = 0,
    /// The frame is claimed and can be initialised ready for sending.
    Created = 1,
    /// The frame is ready to send when the TX loop next runs.
    Sendable = 2,
    /// The frame is being sent over the network interface.
    Sending = 3,
    /// A frame response has been received and is now ready for parsing.
    RxBusy = 5,
    /// Frame response parsing is complete. The frame and its data is ready to be returned in
    /// `Poll::Ready`.
    RxDone = 6,
    /// The frame TX/RX is complete, but the frame is still in use by calling code.
    RxProcessing = 7,
}

#[derive(Debug, Default)]
pub struct PduFrame {
    /// Data length.
    pub len: usize,

    pub index: u8,
    pub command: Command,
    pub flags: PduFlags,
    pub irq: u16,
    pub working_counter: u16,

    pub waker: spin::RwLock<Option<Waker>>,
}

/// An individual frame state, PDU header config, and data buffer.
#[derive(Debug)]
#[repr(C)]
pub struct FrameElement<const N: usize> {
    pub frame: PduFrame,
    status: AtomicFrameState,
    pub buffer: [u8; N],
}

impl<const N: usize> Default for FrameElement<N> {
    fn default() -> Self {
        Self {
            frame: Default::default(),
            status: AtomicFrameState::new(FrameState::None),
            buffer: [0; N],
        }
    }
}

impl<const N: usize> FrameElement<N> {
    unsafe fn buf_ptr(this: NonNull<FrameElement<N>>) -> NonNull<u8> {
        let buf_ptr: *mut [u8; N] = unsafe { addr_of_mut!((*this.as_ptr()).buffer) };
        let buf_ptr: *mut u8 = buf_ptr.cast();
        NonNull::new_unchecked(buf_ptr)
    }

    unsafe fn set_state(this: NonNull<FrameElement<N>>, state: FrameState) {
        let fptr = this.as_ptr();

        (&*addr_of_mut!((*fptr).status)).store(state, Ordering::Release);
    }

    unsafe fn swap_state(
        this: NonNull<FrameElement<N>>,
        from: FrameState,
        to: FrameState,
    ) -> Result<NonNull<FrameElement<N>>, FrameState> {
        let fptr = this.as_ptr();

        (&*addr_of_mut!((*fptr).status)).compare_exchange(
            from,
            to,
            Ordering::AcqRel,
            Ordering::Relaxed,
        )?;

        // If we got here, it's ours.
        Ok(this)
    }

    /// Attempt to clame a frame element as CREATED. Succeeds if the selected FrameElement is
    /// currently in the NONE state.
    pub unsafe fn claim_created(
        this: NonNull<FrameElement<N>>,
    ) -> Result<NonNull<FrameElement<N>>, PduError> {
        Self::swap_state(this, FrameState::None, FrameState::Created).map_err(|e| {
            log::error!(
                "Failed to claim frame: status is {:?}, expected {:?}",
                e,
                FrameState::None
            );

            PduError::SwapState
        })
    }

    pub unsafe fn claim_sending(
        this: NonNull<FrameElement<N>>,
    ) -> Option<NonNull<FrameElement<N>>> {
        Self::swap_state(this, FrameState::Sendable, FrameState::Sending).ok()
    }

    pub unsafe fn claim_receiving(
        this: NonNull<FrameElement<N>>,
    ) -> Option<NonNull<FrameElement<N>>> {
        Self::swap_state(this, FrameState::Sending, FrameState::RxBusy).ok()
    }
}

// Used to store a FrameElement with erased const generics
#[derive(Debug)]
pub struct FrameBox<'a> {
    pub frame: NonNull<FrameElement<0>>,
    pub _lifetime: PhantomData<&'a mut FrameElement<0>>,
}

// TODO: Un-pub all
impl<'a> FrameBox<'a> {
    pub unsafe fn replace_waker(&self, waker: Waker) {
        (&*addr_of!((*self.frame.as_ptr()).frame.waker))
            .try_write()
            .expect("Contention replace_waker")
            .replace(waker);
    }

    pub unsafe fn take_waker(&self) -> Option<Waker> {
        (&*addr_of!((*self.frame.as_ptr()).frame.waker))
            .try_write()
            .expect("Contention take_waker")
            .take()
    }

    pub unsafe fn frame(&self) -> &PduFrame {
        unsafe { &*addr_of!((*self.frame.as_ptr()).frame) }
    }

    pub unsafe fn frame_mut(&self) -> &mut PduFrame {
        unsafe { &mut *addr_of_mut!((*self.frame.as_ptr()).frame) }
    }

    unsafe fn buf_len(&self) -> usize {
        self.frame().len
    }

    pub unsafe fn frame_and_buf(&self) -> (&PduFrame, &[u8]) {
        let buf_ptr = unsafe { addr_of!((*self.frame.as_ptr()).buffer).cast::<u8>() };
        let buf = unsafe { core::slice::from_raw_parts(buf_ptr, self.buf_len()) };
        let frame = unsafe { &*addr_of!((*self.frame.as_ptr()).frame) };
        (frame, buf)
    }

    pub unsafe fn frame_and_buf_mut(&mut self) -> (&mut PduFrame, &mut [u8]) {
        let buf_ptr = unsafe { addr_of_mut!((*self.frame.as_ptr()).buffer).cast::<u8>() };
        let buf = unsafe { core::slice::from_raw_parts_mut(buf_ptr, self.buf_len()) };
        let frame = unsafe { &mut *addr_of_mut!((*self.frame.as_ptr()).frame) };

        (frame, buf)
    }

    pub unsafe fn buf(&self) -> &[u8] {
        let ptr = FrameElement::<0>::buf_ptr(self.frame);
        core::slice::from_raw_parts(ptr.as_ptr(), self.buf_len())
    }

    pub unsafe fn buf_mut(&mut self) -> &mut [u8] {
        let ptr = FrameElement::<0>::buf_ptr(self.frame);
        core::slice::from_raw_parts_mut(ptr.as_ptr(), self.buf_len())
    }
}

#[derive(Debug)]
pub struct CreatedFrame<'a> {
    pub inner: FrameBox<'a>,
}

impl<'a> CreatedFrame<'a> {
    pub fn mark_sendable(self) -> ReceiveFrameFut<'a> {
        unsafe {
            FrameElement::set_state(self.inner.frame, FrameState::Sendable);
        }
        ReceiveFrameFut {
            frame: Some(self.inner),
        }
    }

    pub fn buf_mut(&mut self) -> &mut [u8] {
        unsafe { self.inner.buf_mut() }
    }
}

#[derive(Debug)]
pub struct SendableFrame<'a> {
    inner: FrameBox<'a>,
}

impl<'a> SendableFrame<'a> {
    pub fn new(inner: FrameBox<'a>) -> Self {
        Self { inner }
    }

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

#[derive(Debug)]
pub struct ReceivingFrame<'a> {
    pub inner: FrameBox<'a>,
}

impl<'a> ReceivingFrame<'a> {
    pub fn mark_received(self) -> Result<(), Error> {
        let frame = unsafe { self.inner.frame() };

        log::trace!("Frame and buf mark_received");

        log::trace!("Mark received, waker is {:?}", frame.waker);

        let waker = unsafe { self.inner.take_waker() }.ok_or_else(|| {
            log::error!(
                "Attempted to wake frame #{} with no waker, possibly caused by timeout",
                frame.index
            );

            PduError::InvalidFrameState
        })?;

        unsafe {
            FrameElement::set_state(self.inner.frame, FrameState::RxDone);
        }

        waker.wake();

        Ok(())
    }

    pub fn buf_mut(&mut self) -> &mut [u8] {
        unsafe { self.inner.buf_mut() }
    }

    pub fn reset_readable(self) {
        unsafe { FrameElement::set_state(self.inner.frame, FrameState::None) }
    }

    pub fn index(&self) -> u8 {
        unsafe { self.inner.frame() }.index
    }

    pub fn command(&self) -> Command {
        unsafe { self.inner.frame() }.command
    }
}

pub struct ReceiveFrameFut<'sto> {
    frame: Option<FrameBox<'sto>>,
}

impl<'sto> Future for ReceiveFrameFut<'sto> {
    type Output = Result<ReceivedFrame<'sto>, Error>;

    fn poll(
        mut self: core::pin::Pin<&mut Self>,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<Self::Output> {
        // return Poll::Ready(Err(PduError::InvalidFrameState.into()));

        log::trace!("Poll fut");

        let rxin = match self.frame.take() {
            Some(r) => r,
            None => return Poll::Ready(Err(PduError::InvalidFrameState.into())),
        };

        let swappy = unsafe {
            FrameElement::swap_state(rxin.frame, FrameState::RxDone, FrameState::RxProcessing)
        };

        log::trace!("Swappy {:?}", swappy);

        let was = match swappy {
            Ok(_frame_element) => {
                log::trace!("Frame future is ready");
                return Poll::Ready(Ok(ReceivedFrame { inner: rxin }));
            }
            Err(e) => e,
        };

        log::trace!("Was {:?}", was);

        match was {
            FrameState::Sendable | FrameState::Sending => {
                unsafe { rxin.replace_waker(cx.waker().clone()) };

                self.frame = Some(rxin);

                Poll::Pending
            }
            _ => Poll::Ready(Err(PduError::InvalidFrameState.into())),
        }
    }
}

#[derive(Debug)]
pub struct ReceivedFrame<'sto> {
    inner: FrameBox<'sto>,
}

impl<'sto> ReceivedFrame<'sto> {
    pub fn working_counter(&self) -> u16 {
        unsafe { self.inner.frame() }.working_counter
    }

    pub fn wkc(self, expected: u16, context: &'static str) -> Result<Self, Error> {
        let wkc = self.working_counter();

        if wkc == expected {
            Ok(self)
        } else {
            Err(Error::WorkingCounter {
                expected,
                received: wkc,
                context: Some(context),
            })
        }
    }

    pub fn data(&'sto self) -> &'sto [u8] {
        unsafe { self.inner.buf() }
    }

    pub fn to_owned<T>(self) -> Result<PduResponse<T>, PduError>
    where
        T: PduRead,
    {
        let res = T::try_from_slice(self.data()).map_err(|e| {
            log::error!(
                "PDU data decode: {:?}, T: {} data {:?}",
                e,
                type_name::<T>(),
                self.data()
            );

            PduError::Decode
        })?;

        Ok((res, self.working_counter()))
    }
}

impl<'sto> Drop for ReceivedFrame<'sto> {
    fn drop(&mut self) {
        log::trace!("Drop frame element");

        unsafe { FrameElement::set_state(self.inner.frame, FrameState::None) }
    }
}

pub type PduResponse<T> = (T, u16);
