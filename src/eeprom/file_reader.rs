//! An EEPROM reader backed by an EEPROM image file instead of a real device.
//!
//! Useful for debugging and unit testing. These items should not be used in production as they
//! contain quite a few panics, unwraps and poor assumptions.

use crate::{eeprom::EepromDataProvider, error::Error};
use std::{
    fs::File,
    io::{Read, Seek},
    path::PathBuf,
};

pub struct EepromFile<const CHUNK: usize> {
    path: PathBuf,
    file: File,
    buf: [u8; CHUNK],
}

impl<const CHUNK: usize> Clone for EepromFile<CHUNK> {
    fn clone(&self) -> Self {
        Self {
            path: self.path.clone(),
            file: File::open(&self.path).expect("Could not open EEPROM file in clone"),
            buf: self.buf.clone(),
        }
    }
}

impl EepromFile<8> {
    /// Create an EEPROM file reader that returns chunks of 8 bytes.
    // Allow unused as this is only used in unit tests.
    #[allow(unused)]
    pub fn new(path: impl Into<PathBuf>) -> Self {
        let path = path.into();

        Self {
            file: File::open(&path).expect("Could not open EEPROM file"),
            path,
            buf: [0u8; 8],
        }
    }
}

impl EepromFile<4> {
    // Allow unused as this is only used in unit tests.
    #[allow(unused)]
    pub fn new_short(path: impl Into<PathBuf>) -> Self {
        let path = path.into();

        Self {
            file: File::open(&path).expect("Could not open EEPROM file"),
            path,
            buf: [0u8; 4],
        }
    }
}

impl<const CHUNK: usize> EepromDataProvider for EepromFile<CHUNK> {
    async fn read_chunk(
        &mut self,
        start_word: u16,
    ) -> Result<impl core::ops::Deref<Target = [u8]>, Error> {
        let file_len = self.file.metadata().unwrap().len() as usize;

        self.file
            .seek(std::io::SeekFrom::Start(u64::from(start_word) * 2))
            .expect("Bad seek!");

        // Make sure a partial read off the end of the file is ok, e.g. 8 byte buffer but 4 byte
        // read.
        let buf_len = self.buf.len().min(file_len - usize::from(start_word * 2));

        let mut buf = &mut self.buf[0..buf_len];

        assert!(buf_len == 4 || buf_len == 8);

        self.file
            .read_exact(&mut buf)
            .expect("Could not read from EEPROM file");

        Ok(buf)
    }
}

// #[derive(Debug)]
// pub struct EepromFileHandle<const CHUNK: usize> {
//     file: File,
//     chunk_len: usize,
//     buf: [u8; CHUNK],
// }

// impl<const CHUNK: usize> ChunkReaderTraitLol for EepromFileHandle<CHUNK> {
//     async fn read_chunk(
//         &mut self,
//         start_word: u16,
//     ) -> Result<impl core::ops::Deref<Target = [u8]>, Error> {
//         self.file
//             .seek(std::io::SeekFrom::Start(u64::from(start_word) * 2));

//         self.file
//             .read_exact(&mut self.buf)
//             .expect("Could not read from EEPROM file");

//         Ok(self.buf.as_slice())
//     }

//     fn seek_to_word(&mut self, word: u16) {
//         todo!()
//     }
// }

// impl embedded_io_async::Read for EepromFileHandle {
//     async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
//         self.file.read(buf).map_err(|e| {
//             fmt::error!("File read error: {}", e);

//             Error::Internal
//         })
//     }
// }

// impl embedded_io_async::Seek for EepromFileHandle {
//     async fn seek(&mut self, pos_words: embedded_io_async::SeekFrom) -> Result<u64, Self::Error> {
//         // EEPROM addresses are all words, so we must convert into bytes to correctly offset into
//         // the file.
//         let pos_bytes = match pos_words {
//             embedded_io_async::SeekFrom::Start(start) => std::io::SeekFrom::Start(start * 2),
//             embedded_io_async::SeekFrom::End(start) => std::io::SeekFrom::End(start * 2),
//             embedded_io_async::SeekFrom::Current(current) => {
//                 std::io::SeekFrom::Current(current * 2)
//             }
//         };

//         self.file.seek(pos_bytes).map_err(|e| {
//             fmt::error!("File seek error: {}", e);

//             Error::Internal
//         })
//     }
// }

// impl embedded_io_async::ErrorType for EepromFileHandle {
//     type Error = Error;
// }
