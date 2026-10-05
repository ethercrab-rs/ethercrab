//! Items required for running in `std` environments.

#[cfg(all(target_os = "linux", feature = "io-uring"))]
mod io_uring;
#[cfg(unix)]
mod unix;
#[cfg(target_os = "windows")]
mod windows;
#[cfg(all(target_os = "linux", feature = "xdp"))]
mod xdp;

use std::{
    sync::Arc,
    task::Wake,
    thread::{self, Thread},
};

#[cfg(target_os = "windows")]
pub use self::windows::{ethercat_now, tx_rx_task_blocking};
#[cfg(unix)]
pub use unix::{ethercat_now, tx_rx_task};
// io_uring is Linux-only
#[cfg(all(target_os = "linux", feature = "io-uring"))]
pub use io_uring::tx_rx_task_io_uring;
#[cfg(all(target_os = "linux", feature = "xdp"))]
pub use xdp::tx_rx_task_xdp;

/// Configuration for the TX/RX tasks.
#[derive(Copy, Clone, Debug, Default)]
pub struct TxRxTaskConfig {
    /// If set to `true`, use a spinloop to wait for packet TX or RX instead of putting the thread
    /// to sleep.
    ///
    /// If enabled, this option will peg a CPU core to 100% usage but may improve latency and
    /// jitter. It is recommended to pin it to a core using
    /// [`thread_priority`](https://docs.rs/thread-priority/latest/x86_64-pc-windows-msvc/thread_priority/index.html)
    /// or similar.
    ///
    /// Windows only, ignored on other platforms.
    pub spinloop: bool,

    /// If set to `true`, accept received frames with an unchanged source MAC address.
    ///
    /// May be required by some interfaces.
    /// See: <https://github.com/Beckhoff/CCAT/issues/16>
    ///
    /// Linux only, ignored on other platforms.
    pub accept_own_source_mac: bool,
}

struct ParkSignal {
    current_thread: Thread,
}

impl ParkSignal {
    fn new() -> Self {
        Self {
            current_thread: thread::current(),
        }
    }

    fn wait(&self) {
        thread::park();
    }

    // fn wait_timeout(&self, timeout: Duration) {
    //     thread::park_timeout(timeout)
    // }
}

impl Wake for ParkSignal {
    fn wake(self: Arc<Self>) {
        self.current_thread.unpark();
    }
}
