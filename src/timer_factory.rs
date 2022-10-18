use crate::error::Error;
use core::time::Duration;
use embassy_futures::select::{select, Either};

pub trait TimerFactory: core::future::Future + Unpin {
    fn timer(duration: core::time::Duration) -> Self;
}

impl TimerFactory for smol::Timer {
    fn timer(duration: core::time::Duration) -> Self {
        Self::after(duration)
    }
}

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
    pub wait_loop_delay: Duration,

    /// How long to wait for a slave mailbox to become ready.
    pub mailbox: Duration,
}

impl Timeouts {
    pub(crate) async fn loop_tick<TIMEOUT>(&self)
    where
        TIMEOUT: TimerFactory,
    {
        TIMEOUT::timer(self.wait_loop_delay).await;
    }
}

impl Default for Timeouts {
    fn default() -> Self {
        Self {
            state_transition: Duration::from_millis(5000),
            pdu: Duration::from_micros(30_000),
            eeprom: Duration::from_millis(10),
            wait_loop_delay: Duration::from_millis(0),
            mailbox: Duration::from_millis(10),
        }
    }
}

pub(crate) async fn timeout<TIMEOUT, O, F>(timeout: Duration, future: F) -> Result<O, Error>
where
    TIMEOUT: TimerFactory,
    F: core::future::Future<Output = Result<O, Error>>,
{
    pin!(future);

    match select(future, TIMEOUT::timer(timeout)).await {
        Either::First(res) => res,
        Either::Second(_timeout) => Err(Error::Timeout),
    }
}
