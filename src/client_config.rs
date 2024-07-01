//! Configuration passed to [`Client`](crate::Client).

/// Configuration passed to [`Client`](crate::Client).
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct ClientConfig {
    /// The number of `FRMW` packets to send during the static phase of Distributed Clocks (DC)
    /// synchronisation.
    ///
    /// Defaults to 10000.
    ///
    /// If this is set to zero, no static sync will be performed.
    pub dc_static_sync_iterations: u32,

    /// EtherCAT packet (PDU) network retry behaviour.
    pub retry_behaviour: RetryBehaviour,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            dc_static_sync_iterations: 10_000,
            retry_behaviour: RetryBehaviour::default(),
        }
    }
}

/// Network communication retry policy.
///
/// Retries will be performed at the rate defined by [`Timeouts::pdu`](crate::Timeouts::pdu).
#[derive(Debug, Copy, Clone, Default, PartialEq, Eq)]
pub enum RetryBehaviour {
    /// Do not attempt to retry timed out packet sends (default).
    ///
    /// If this option is chosen, any timeouts will raise an
    /// [`Error::Timeout`](crate::error::Error::Timeout).
    #[default]
    None,

    /// Attempt to resend a PDU up to `N` times, then raise an
    /// [`Error::Timeout`](crate::error::Error::Timeout).
    Count(usize),

    /// Attempt to resend the PDU forever(*).
    ///
    /// Note that this can soft-lock a program if for example the EtherCAT network cable is removed
    /// as EtherCrab will attempt to resend the packet forever. It may be preferable to use
    /// [`RetryBehaviour::Count`] to set an upper bound on retries.
    ///
    /// (*) Forever in this case means a retry count of `usize::MAX`.
    Forever,
}

impl RetryBehaviour {
    pub(crate) const fn retry_count(&self) -> usize {
        match self {
            // Try at least once when used in a range like `for _ in 0..<counts>`.
            RetryBehaviour::None => 0,
            RetryBehaviour::Count(n) => *n,
            RetryBehaviour::Forever => usize::MAX,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retry_count_sanity_check() {
        assert_eq!(RetryBehaviour::None.retry_count(), 0);
        assert_eq!(RetryBehaviour::Count(10).retry_count(), 10);
        assert_eq!(RetryBehaviour::Forever.retry_count(), usize::MAX);
    }
}
