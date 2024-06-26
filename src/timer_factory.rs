use crate::{error::Error, Client, WrappedRead};
use core::{future::Future, pin::Pin, task::Poll, time::Duration};
use ethercrab_wire::{EtherCrabWireRead, EtherCrabWireSized};
use futures_lite::FutureExt;

#[cfg(not(feature = "std"))]
pub(crate) type Timer = embassy_time::Timer;
#[cfg(feature = "std")]
pub(crate) type Timer = async_io::Timer;

#[cfg(not(feature = "std"))]
pub(crate) fn timer(duration: Duration) -> Timer {
    embassy_time::Timer::after(embassy_time::Duration::from_micros(
        duration.as_micros() as u64
    ))
}

#[cfg(feature = "std")]
pub(crate) fn timer(duration: Duration) -> Timer {
    async_io::Timer::after(duration)
}

pub(crate) trait IntoTimeout<O> {
    fn timeout(self, timeout: Duration) -> TimeoutFuture<impl Future<Output = Result<O, Error>>>;
}

impl<T, O> IntoTimeout<O> for T
where
    T: Future<Output = Result<O, Error>>,
{
    fn timeout(self, timeout: Duration) -> TimeoutFuture<T> {
        let timeout = timer(timeout);

        TimeoutFuture { f: self, timeout }
    }
}

pub(crate) struct TimeoutFuture<F> {
    f: F,

    timeout: Timer,
}

impl<F, O> Future for TimeoutFuture<F>
where
    F: Future<Output = Result<O, Error>>,
{
    type Output = Result<O, Error>;

    fn poll(self: Pin<&mut Self>, cx: &mut core::task::Context<'_>) -> Poll<Self::Output> {
        let this = unsafe { self.get_unchecked_mut() };
        let timeout = unsafe { Pin::new_unchecked(&mut this.timeout) };
        let f = unsafe { Pin::new_unchecked(&mut this.f) };

        if timeout.poll(cx).is_ready() {
            return Poll::Ready(Err(Error::Timeout));
        }

        if let Poll::Ready(x) = f.poll(cx) {
            return Poll::Ready(x);
        }

        Poll::Pending
    }
}

/// Timeout configuration for the EtherCrab master.
#[derive(Copy, Clone, Debug)]
pub struct Timeouts {
    /// How long to wait for a slave state change, e.g. SAFE-OP to OP.
    ///
    /// This timeout is global for all state transitions.
    pub state_transition: Duration,

    /// How long to wait for a PDU response.
    pub pdu: Duration,

    /// How long to wait for a single EEPROM operation.
    pub eeprom: Duration,

    /// Polling duration of wait loops.
    ///
    /// Some operations require repeatedly reading something from a slave until a value changes.
    /// This duration specifies the wait time between polls.
    ///
    /// This defaults to a timeout of 0 to keep latency to a minimum.
    pub wait_loop_delay: Duration,

    /// How long to wait for a slave mailbox to become ready.
    pub mailbox_echo: Duration,

    /// How long to wait for a response to be read from the slave's response mailbox.
    pub mailbox_response: Duration,
}

impl Timeouts {
    pub(crate) async fn loop_tick(&self) {
        timer(self.wait_loop_delay).await;
    }
}

impl Default for Timeouts {
    fn default() -> Self {
        Self {
            state_transition: Duration::from_millis(5000),
            pdu: Duration::from_micros(30_000),
            eeprom: Duration::from_millis(10),
            wait_loop_delay: Duration::from_millis(0),
            mailbox_echo: Duration::from_millis(100),
            mailbox_response: Duration::from_millis(1000),
        }
    }
}

pub(crate) async fn poll_tick<T>(
    client: &'_ Client<'_>,
    command: WrappedRead,
    pred: impl Fn(&T) -> bool,
    timeout: Duration,
) -> Result<T, Error>
where
    T: EtherCrabWireRead + EtherCrabWireSized,
{
    let mut timeout = timer(timeout);
    let mut cmd_fut = core::pin::pin!(command.receive::<T>(client));
    let mut tick_fut: Pin<&mut Option<Timer>> = core::pin::pin!(None);

    core::future::poll_fn(|ctx| {
        // Whole request timeout
        if let Poll::Ready(_) = timeout.poll(ctx) {
            return Poll::Ready(Err(Error::Timeout));
        }

        // Loop tick timer
        if let Some(mut f) = tick_fut.take() {
            match f.poll(ctx) {
                Poll::Ready(_) => cmd_fut.set(command.receive::<T>(client)),
                Poll::Pending => {
                    tick_fut.set(Some(f));

                    return Poll::Pending;
                }
            }
        }

        // See if we've received a response yet
        match cmd_fut.as_mut().poll(ctx) {
            Poll::Ready(result) => {
                let result = match result {
                    Ok(res) => res,
                    Err(e) => return Poll::Ready(Err(e)),
                };

                match pred(&result) {
                    // Condition is satisfied; we can quit the loop now
                    true => Poll::Ready(Ok(result)),
                    // Set a timer to wait for a bit before sending the command again
                    false => {
                        let mut t = timer(client.timeouts.wait_loop_delay);
                        // Poll once to register with executor
                        let _ = t.poll(ctx);
                        tick_fut.set(Some(t));

                        Poll::Pending
                    }
                }
            }
            // Command hasn't received response yet
            Poll::Pending => Poll::Pending,
        }
    })
    .await
}
