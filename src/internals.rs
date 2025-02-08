//! Expose various unstable EtherCrab internals.
//!
//! Anything exported by this module should be considered unstable and may change at any time.

pub use crate::eeprom::device_provider::DeviceEeprom;
pub use crate::eeprom::EepromDataProvider;
pub use crate::eeprom::EepromRange;
pub use crate::ethernet::{EthernetAddress, EthernetFrame};
