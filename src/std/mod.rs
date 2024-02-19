//! Items required for running in `std` environments.

#[cfg(target_os = "linux")]
mod io_uring;
#[cfg(unix)]
mod unix;
#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "windows")]
pub use self::windows::tx_rx_task;
#[cfg(unix)]
pub use unix::tx_rx_task;
// io_uring is Linux-only
#[cfg(target_os = "linux")]
pub use io_uring::tx_rx_task_io_uring;
