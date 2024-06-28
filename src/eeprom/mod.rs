use core::ops::Deref;

use crate::{
    error::{EepromError, Error},
    fmt,
};
use embedded_io_async::{ErrorType, Read, ReadExactError};

pub mod device_reader;
pub mod types;

#[cfg(feature = "std")]
pub mod file_reader;

/// A data source for EEPROM reads.
pub trait EepromDataProvider: Clone {
    /// Read a chunk of either 4 or 8 bytes from the backing store.
    #[cfg_attr(feature = "__internals", allow(async_fn_in_trait))]
    async fn read_chunk(&mut self, start_word: u16) -> Result<impl Deref<Target = [u8]>, Error>;

    /// Attempt to clear any errors in the EEPROM source.
    #[cfg_attr(feature = "__internals", allow(async_fn_in_trait))]
    async fn clear_errors(&self) -> Result<(), Error>;
}

impl embedded_io_async::Error for Error {
    fn kind(&self) -> embedded_io_async::ErrorKind {
        // TODO: match()?
        embedded_io_async::ErrorKind::Other
    }
}

impl From<ReadExactError<Error>> for Error {
    fn from(value: ReadExactError<Error>) -> Self {
        match value {
            ReadExactError::UnexpectedEof => Error::Eeprom(EepromError::SectionOverrun),
            ReadExactError::Other(e) => e,
        }
    }
}

/// An abstraction over a provider of EEPROM bytes that only allows a certain range to be read.
///
/// The provider `P` should be as simple as possible, simply returning chunks of data either 4 or 8
/// bytes long. Other lengths are not tested as the EtherCAT specification requires/supports only 4
/// or 8 byte SII reads.
#[derive(Debug)]
pub struct ChunkReader<P> {
    reader: P,

    /// Current logical byte position in the entire address space.
    ///
    /// This is the last byte that was returned to the caller by the reader, and should be used as a
    /// base for skip offsets.
    pos: u16,

    /// The last byte address we're allowed to access.
    end: u16,
}

impl<P> ChunkReader<P>
where
    P: EepromDataProvider,
{
    /// Create a new `ChunkReader`.
    pub fn new(reader: P, start_word: u16, len_words: u16) -> Self {
        Self {
            reader,
            pos: start_word * 2,
            end: start_word * 2 + len_words * 2,
        }
    }

    /// Skip N bytes (NOT words) ahead of the current position.
    pub fn skip_ahead_bytes(&mut self, skip: u16) -> Result<(), Error> {
        fmt::trace!(
            "Skip EEPROM from pos {:#06x} by {} bytes to {:#06x}",
            self.pos,
            skip,
            self.pos + skip,
        );

        if self.pos + skip >= self.end {
            return Err(Error::Eeprom(EepromError::SectionOverrun));
        }

        self.pos += skip;

        Ok(())
    }

    /// Read a single byte.
    pub async fn read_byte(&mut self) -> Result<u8, Error> {
        self.reader.clear_errors().await?;

        let res = self.reader.read_chunk(self.pos / 2).await?;

        // pos is in bytes, but we're reading words. If the current pos is odd, we must skip the
        // first byte of the returned word.
        let skip = usize::from(self.pos % 2);

        // Advance by one byte
        self.pos += 1;

        Ok(res[skip])
    }
}

impl<P> Read for ChunkReader<P>
where
    P: EepromDataProvider,
{
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        fmt::trace!("Read EEPROM chunk from byte {:#06x}", self.pos);

        let requested_read_len = buf.len();

        let max_read = usize::from(self.end - self.pos);

        let mut bytes_read = 0;

        // The read pointer has reached the end of the chunk
        if max_read == 0 {
            return Ok(0);
        }

        // We can't read past the end of the chunk, so clamp the buffer's length to the remaining
        // part of the chunk if necessary.
        let mut buf = &mut buf[0..requested_read_len.min(max_read)];

        self.reader.clear_errors().await?;

        while !buf.is_empty() {
            let res = self.reader.read_chunk(self.pos / 2).await?;

            let chunk = &*res;

            // If position is odd, we must skip the first received byte as the reader operates on
            // WORD addresses.
            let skip = usize::from(self.pos % 2);

            // Fix any odd addressing offsets
            let chunk = &chunk[skip..];

            // Buffer is full after reading this chunk into it. We're done.
            if buf.len() < chunk.len() {
                let (chunk, _rest) = chunk.split_at(buf.len());

                bytes_read += chunk.len();
                self.pos += chunk.len() as u16;

                buf.copy_from_slice(chunk);

                break;
            }

            bytes_read += chunk.len();
            self.pos += chunk.len() as u16;

            // Buffer is not full. Write another chunk into the beginning of it.
            let (buf_start, buf_rest) = buf.split_at_mut(chunk.len());

            buf_start.copy_from_slice(chunk);

            fmt::trace!("--> Buf for next iter {}", buf_rest.len());

            // Shorten the buffer so the next write starts after the one we just did.
            buf = buf_rest;
        }

        fmt::trace!(
            "--> Done. Read {} of requested {} B, pos is now {:#06x}",
            bytes_read,
            requested_read_len,
            self.pos
        );

        Ok(bytes_read)
    }
}

impl<P> ErrorType for ChunkReader<P> {
    type Error = Error;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eeprom::file_reader::EepromFile;

    #[tokio::test]
    async fn skip_past_end() {
        let _ = env_logger::builder().is_test(true).try_init();

        let mut r = ChunkReader::new(EepromFile::new("dumps/eeprom/akd.hex"), 0, 32);

        // Current position is zero, so 32 words = 64 bytes = ok
        assert_eq!(r.skip_ahead_bytes(63), Ok(()), "63 bytes");

        let mut r = ChunkReader::new(EepromFile::new("dumps/eeprom/akd.hex"), 0, 32);

        // Off by one errors are always fun
        assert_eq!(
            r.skip_ahead_bytes(64),
            Err(Error::Eeprom(EepromError::SectionOverrun)),
            "64 bytes"
        );

        let mut r = ChunkReader::new(EepromFile::new("dumps/eeprom/akd.hex"), 0, 32);

        // 65 is one byte off the end
        assert_eq!(
            r.skip_ahead_bytes(65),
            Err(Error::Eeprom(EepromError::SectionOverrun)),
            "65 bytes"
        );

        let mut r = ChunkReader::new(EepromFile::new("dumps/eeprom/akd.hex"), 0, 32);

        // Madness
        assert_eq!(
            r.skip_ahead_bytes(10000),
            Err(Error::Eeprom(EepromError::SectionOverrun)),
            "10000 bytes"
        );
    }

    #[tokio::test]
    async fn read_single_bytes() {
        let _ = env_logger::builder().is_test(true).try_init();

        let mut r = ChunkReader::new(EepromFile::new("dumps/eeprom/el2828.hex"), 0, 32);

        let expected = [
            0x04u8, 0x01, 0x00, 0x00, 0x00, 0x00, 0xff, 0x00, // First 8
            0x00u8, 0x00, 0x00, 0x00, 0x00, 0x00, 0xe2, 0x00, // Second 8
        ];

        let actual = vec![
            // First 8
            r.read_byte().await.unwrap(),
            r.read_byte().await.unwrap(),
            r.read_byte().await.unwrap(),
            r.read_byte().await.unwrap(),
            r.read_byte().await.unwrap(),
            r.read_byte().await.unwrap(),
            r.read_byte().await.unwrap(),
            r.read_byte().await.unwrap(),
            // Second 8
            r.read_byte().await.unwrap(),
            r.read_byte().await.unwrap(),
            r.read_byte().await.unwrap(),
            r.read_byte().await.unwrap(),
            r.read_byte().await.unwrap(),
            r.read_byte().await.unwrap(),
            r.read_byte().await.unwrap(),
            r.read_byte().await.unwrap(),
        ];

        assert_eq!(
            expected,
            actual.as_slice(),
            "Expected:\n{:#04x?}\n\nActual: \n{:#04x?}",
            expected,
            actual
        );
    }
}
