//! An EEPROM reader backed by an EEPROM image file instead of a real device.
//!
//! Useful for debugging and unit testing.

use crate::eeprom::{EepromBlock, EepromDataProvider};
use std::path::PathBuf;

pub struct EepromFile {
    //
}

impl EepromFile {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {}
    }
}

impl EepromDataProvider for EepromFile {
    type Handle = EepromFileCategory;

    async fn category(
        &self,
        category: super::types::CategoryType,
    ) -> Result<Option<Self::Handle>, crate::error::Error> {
        todo!()
    }

    fn address(&self, address: u16, len_bytes: u16) -> Self::Handle {
        todo!()
    }
}

struct EepromFileCategory {
    //
}
