//! Expose various unstable EtherCrab internals.
//!
//! Anything exported by this module should be considered unstable and may change at any time.

pub use crate::eeprom::device_provider::DeviceEeprom;
pub use crate::eeprom::ChunkReader;
pub use crate::eeprom::EepromDataProvider;
pub use crate::ethernet::{EthernetAddress, EthernetFrame};
