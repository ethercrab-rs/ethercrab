//! Items required for running in `std` environments.

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::tx_rx_task;

#[cfg(unix)]
mod unix;
#[cfg(unix)]
pub use unix::tx_rx_task;
