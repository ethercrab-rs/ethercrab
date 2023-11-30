//! Items required for running in `std` environments.

#[cfg(unix)]
mod unix;
#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "windows")]
pub use self::windows::tx_rx_task;
#[cfg(unix)]
pub use unix::tx_rx_task;
