//! An EEPROM reader backed by an EEPROM image file instead of a real device.
//!
//! Useful for debugging and unit testing. These items should not be used in production as they
//! contain quite a few panics, unwraps and poor assumptions.

use crate::{eeprom::EepromDataProvider, error::Error};
use std::io::{BufReader, Cursor, Read, Seek};

pub struct EepromFile<const CHUNK: usize> {
    file_len: usize,
    bytes: BufReader<Cursor<&'static [u8]>>,
    buf: [u8; CHUNK],
}

impl<const CHUNK: usize> Clone for EepromFile<CHUNK> {
    fn clone(&self) -> Self {
        Self {
            bytes: BufReader::new(self.bytes.get_ref().clone()),
            file_len: self.file_len,
            buf: self.buf,
        }
    }
}

impl EepromFile<8> {
    /// Create an EEPROM file reader that returns chunks of 8 bytes.
    // Allow unused as this is only used in unit tests.
    #[allow(unused)]
    pub fn new(bytes: &'static [u8]) -> Self {
        Self {
            file_len: bytes.len(),
            bytes: BufReader::new(Cursor::new(bytes)),
            buf: [0u8; 8],
        }
    }
}

impl EepromFile<4> {
    // Allow unused as this is only used in unit tests.
    #[allow(unused)]
    pub fn new_short(bytes: &'static [u8]) -> Self {
        Self {
            file_len: bytes.len(),
            bytes: BufReader::new(Cursor::new(bytes)),
            buf: [0u8; 4],
        }
    }
}

impl<const CHUNK: usize> EepromDataProvider for EepromFile<CHUNK> {
    async fn read_chunk(
        &mut self,
        start_word: u16,
    ) -> Result<impl core::ops::Deref<Target = [u8]>, Error> {
        let file_len = self.file_len;

        self.bytes
            .seek(std::io::SeekFrom::Start(u64::from(start_word) * 2))
            .expect("Bad seek!");

        // Make sure a partial read off the end of the file is ok, e.g. 8 byte buffer but 4 byte
        // read.
        let buf_len = self.buf.len().min(file_len - usize::from(start_word * 2));

        let buf = &mut self.buf[0..buf_len];

        assert!(
            buf_len == 4 || buf_len == 8,
            "Expected buf len of 4 or 8, got {}",
            buf_len
        );

        self.bytes
            .read_exact(buf)
            .expect("Could not read from EEPROM file");

        Ok(buf)
    }

    async fn clear_errors(&self) -> Result<(), Error> {
        Ok(())
    }

    async fn write_chunk(&mut self, start_word: u16, data: &[u8]) -> Result<(), Error> {
        unimplemented!()
    }
}
