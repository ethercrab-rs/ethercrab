//! Items required for running in `std` environments.

#[cfg(any(target_os = "windows", target_os = "macos"))]
mod not_linux;
#[cfg(any(target_os = "windows", target_os = "macos"))]
pub use not_linux::tx_rx_task;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::tx_rx_task;
