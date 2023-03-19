use super::{
    frame_element::{sendable_frame::SendableFrame, FrameBox, FrameElement},
    storage::PduStorageRef,
};
use core::{
    marker::PhantomData,
    ptr::NonNull,
    task::{Poll, Waker},
};

/// Send data frames over a network interface.
pub struct PduTx<'sto> {
    storage: PduStorageRef<'sto>,
}

// SAFETY: We're tied to the lifetime of the backing storage with 'sto.
unsafe impl<'sto> Send for PduTx<'sto> {}
unsafe impl<'sto> Sync for PduTx<'sto> {}

impl<'sto> PduTx<'sto> {
    pub(in crate::pdu_loop) fn new(storage: PduStorageRef<'sto>) -> Self {
        Self { storage }
    }

    fn next_sendable_frame(&self) -> Option<SendableFrame<'sto>> {
        for idx in 0..self.storage.num_frames {
            let frame = unsafe { NonNull::new_unchecked(self.storage.frame_at_index(idx)) };

            let sending = if let Some(frame) = unsafe { FrameElement::claim_sending(frame) } {
                SendableFrame::new(FrameBox {
                    frame,
                    _lifetime: PhantomData,
                })
            } else {
                continue;
            };

            return Some(sending);
        }

        None
    }

    fn set_waker(&self, waker: Waker) {
        let current_waker_guard = self.storage.tx_waker.upgradeable_read();

        if let Some(current_waker) = &*current_waker_guard {
            if !waker.will_wake(current_waker) {
                current_waker_guard.upgrade().replace(waker);
            }
        } else {
            current_waker_guard.upgrade().replace(waker);
        }
    }

    /// Wait for the next sendable frame to become available.
    // NOTE: &mut self so this struct can only be used in one place.
    pub fn next<'fut>(&'fut mut self) -> PduTxFut<'fut> {
        PduTxFut { tx: self }
    }
}

pub struct PduTxFut<'a> {
    tx: &'a PduTx<'a>,
}

impl<'a> core::future::Future for PduTxFut<'a> {
    type Output = SendableFramesIter<'a>;

    fn poll(
        self: core::pin::Pin<&mut Self>,
        ctx: &mut core::task::Context<'_>,
    ) -> Poll<Self::Output> {
        match self.tx.next_sendable_frame() {
            Some(frame) => Poll::Ready(SendableFramesIter::new(self.tx, frame)),
            None => {
                self.tx.set_waker(ctx.waker().clone());

                Poll::Pending
            }
        }
    }
}

pub struct SendableFramesIter<'a> {
    tx: &'a PduTx<'a>,
    first_frame: Option<SendableFrame<'a>>,
    idx: usize,
}

impl<'a> SendableFramesIter<'a> {
    pub fn new(tx: &'a PduTx<'a>, first_frame: SendableFrame<'a>) -> Self {
        Self {
            tx,
            first_frame: Some(first_frame),
            idx: 0,
        }
    }
}

impl<'a> Iterator for SendableFramesIter<'a> {
    type Item = SendableFrame<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        self.first_frame.take().or_else(|| {
            while self.idx < self.tx.storage.num_frames {
                let frame =
                    unsafe { NonNull::new_unchecked(self.tx.storage.frame_at_index(self.idx)) };

                if let Some(frame) = unsafe { FrameElement::claim_sending(frame) } {
                    return Some(SendableFrame::new(FrameBox {
                        frame,
                        _lifetime: PhantomData,
                    }));
                }

                self.idx += 1;
            }

            None
        })
    }
}
