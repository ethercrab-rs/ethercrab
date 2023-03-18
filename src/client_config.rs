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
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            dc_static_sync_iterations: 10_000,
        }
    }
}
