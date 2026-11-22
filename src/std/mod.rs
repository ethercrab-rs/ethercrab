//! Items required for running in `std` environments.

#[cfg(all(target_os = "linux", feature = "io-uring"))]
mod io_uring;
#[cfg(any(feature = "xdp", feature = "io-uring", target_os = "windows"))]
mod park_signal;
#[cfg(unix)]
mod unix;
#[cfg(target_os = "windows")]
mod windows;
#[cfg(all(target_os = "linux", feature = "xdp"))]
mod xdp;

#[cfg(target_os = "windows")]
pub use self::windows::{TxRxTaskConfig, ethercat_now, tx_rx_task_blocking};
#[cfg(unix)]
pub use unix::{ethercat_now, tx_rx_task};
// io_uring is Linux-only
#[cfg(all(target_os = "linux", feature = "io-uring"))]
pub use io_uring::tx_rx_task_io_uring;
#[cfg(all(target_os = "linux", feature = "xdp"))]
pub use xdp::tx_rx_task_xdp;
