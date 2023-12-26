//! An EEPROM reader backed by an EEPROM image file instead of a real device.
//!
//! Useful for debugging and unit testing. These items should not be used in production as they
//! contain quite a few panics and unwraps.

use crate::{eeprom::EepromDataProvider, error::Error, fmt};
use std::{
    fs::File,
    io::{Read, Seek},
    path::PathBuf,
};

pub struct EepromFile {
    path: PathBuf,
}

impl EepromFile {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }
}

impl EepromDataProvider for EepromFile {
    type Provider = EepromFileHandle;

    fn reader(&self) -> Self::Provider {
        let file = File::open(&self.path).expect("Could not open EEPROM file");

        EepromFileHandle { file }
    }
}

#[derive(Debug)]
pub struct EepromFileHandle {
    file: File,
}

impl embedded_io_async::Read for EepromFileHandle {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        self.file.read(buf).map_err(|e| {
            fmt::error!("File read error: {}", e);

            Error::Internal
        })
    }
}

impl embedded_io_async::Seek for EepromFileHandle {
    async fn seek(&mut self, pos: embedded_io_async::SeekFrom) -> Result<u64, Self::Error> {
        self.file.seek(pos.into()).map_err(|e| {
            fmt::error!("File seek error: {}", e);

            Error::Internal
        })
    }
}

impl embedded_io_async::ErrorType for EepromFileHandle {
    type Error = Error;
}
