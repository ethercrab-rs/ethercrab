use crate::error::Error;
use core::{future::Future, pin::Pin, task::Poll, time::Duration};

#[cfg(not(feature = "std"))]
type Timer = embassy_time::Timer;
#[cfg(feature = "std")]
type Timer = async_io::Timer;

#[cfg(not(feature = "std"))]
fn timer(duration: Duration) -> Timer {
    embassy_time::Timer::after(embassy_time::Duration::from_micros(
        duration.as_micros() as u64
    ))
}

#[cfg(feature = "std")]
fn timer(duration: Duration) -> Timer {
    async_io::Timer::after(duration)
}

pub(crate) trait IntoTimeout<O> {
    fn timeout(self, timeout: Duration) -> TimeoutFuture<impl Future<Output = Result<O, Error>>>;
}

impl<T, O> IntoTimeout<O> for T
where
    T: Future<Output = Result<O, Error>>,
{
    fn timeout(self, timeout: Duration) -> TimeoutFuture<impl Future<Output = Result<O, Error>>> {
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
