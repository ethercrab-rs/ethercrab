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
    async fn read_chunk(&mut self, start_word: u16) -> Result<impl Deref<Target = [u8]>, Error>;
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

    /// Extra data that was read from the device but not returned by calls to `read()`.
    cache: heapless::Vec<u8, 8>,

    /// Position of last data that was actually asked for, e.g. the next byte after the current
    /// cache.
    ///
    /// This is WORD based.
    read_pointer: u16,
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
            // len: len_bytes,
            end: start_word * 2 + len_words * 2,
            cache: heapless::Vec::new(),
            read_pointer: start_word,
        }
    }

    /// Skip N bytes (NOT words) ahead of the current position.
    pub async fn skip_ahead_bytes(&mut self, skip: u16) -> Result<(), Error> {
        fmt::trace!(
            "Skip EEPROM from pos {:#06x}, read pointer {:#06x}, by {} bytes to {:#06x}, end {:#06x}",
            self.pos,
            self.read_pointer,
            skip,
            self.pos + skip,
            self.end
        );

        if self.pos + skip >= self.end {
            return Err(Error::Eeprom(EepromError::SectionOverrun));
        }

        self.pos += skip;

        // Round read pointer down to the nearest multiple of two (byte -> word conversion)
        self.read_pointer = self.pos / 2;

        fmt::trace!(
            "--> After skip: pos {:#06x}, read pointer {:#06x}",
            self.pos,
            self.read_pointer,
        );

        // Take next chunk so we can prepopulate the cache for the next read.
        let res = self.reader.read_chunk(self.read_pointer).await?;
        self.read_pointer += res.len() as u16 / 2;

        // If the new logical position is odd, we must discard the first byte of the read chunk as
        // it is word-aligned (i.e. even bytes).
        let discard = usize::from(self.pos % 2);

        let trimmed = &res[discard..];

        #[cfg(not(feature = "defmt"))]
        fmt::trace!(
            "--> Discard {} bytes, cache from {:02x?} -> {:02x?}",
            discard,
            res.deref(),
            trimmed
        );
        #[cfg(feature = "defmt")]
        fmt::trace!(
            "--> Discard {} bytes, cache from {=[u8]} -> {=[u8]}",
            discard,
            res.deref(),
            trimmed
        );

        self.cache = fmt::unwrap!(heapless::Vec::from_slice(trimmed));

        Ok(())
    }

    /// Read a single byte.
    pub async fn read_byte(&mut self) -> Result<u8, Error> {
        let mut buf = [0u8; 1];

        self.read_exact(&mut buf).await?;

        Ok(buf[0])
    }
}

impl<P> Read for ChunkReader<P>
where
    P: EepromDataProvider,
{
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        fmt::trace!(
            "Read EEPROM chunk from read pointer byte {:#06x}",
            self.read_pointer
        );

        let requested_read_len = buf.len();

        let max_read = usize::from(self.end - self.pos);

        let mut bytes_read = 0;

        // The read pointer has reached the end of the chunk
        if max_read == 0 {
            return Ok(0);
        }

        let buf_len = buf.len();

        // We can't read past the end of the chunk, so clamp the buffer's length to the remaining
        // part of the chunk if necessary.
        let buf = &mut buf[0..buf_len.min(max_read)];

        // If there's any current cached data, split off existing cache buffer and write into result
        // buf
        let (cached, cache_rest) = self.cache.split_at(buf.len().min(self.cache.len()));

        if !cached.is_empty() {
            #[cfg(not(feature = "defmt"))]
            fmt::trace!(
                "--> Cache has existing data: {:02x?} : {:02x?}",
                cached,
                cache_rest
            );
            #[cfg(feature = "defmt")]
            fmt::trace!(
                "--> Cache has existing data: {=[u8]} : {=[u8]}",
                cached,
                cache_rest
            );
        }

        // Make sure the bit of the buffer we're copying into matches the cache chunk length
        let (start, mut buf) = buf.split_at_mut(cached.len());

        start.copy_from_slice(cached);

        // Move byte position further along the EEPROM address space by however much cache we've
        // taken.
        bytes_read += cached.len();

        // Re-store any remaining cached data ready for the next read.
        //
        // This should never panic as the source data is the same type, so it can be at most exactly
        // the same length, or shorter.
        self.cache = fmt::unwrap!(heapless::Vec::from_slice(cache_rest));

        // If there is more to read, read chunks from provider until buffer is full. The remaining
        // `buf` will be empty if the cache completely fulfilled our request so this loop won't
        // execute in that case.
        while buf.len() > 0 {
            let res = self.reader.read_chunk(self.read_pointer).await?;
            self.read_pointer += res.len() as u16 / 2;

            let chunk = res.deref();

            // Buffer is full after reading this chunk into it. We're done.
            if buf.len() < chunk.len() {
                let (chunk, into_cache) = chunk.split_at(buf.len());

                bytes_read += chunk.len();

                buf.copy_from_slice(chunk);

                // Unwrap: logic bug! The returned chunk must be either 4 or 8 bytes, and we have a
                // vec of 8 bytes in length, so this should not fail if everything is correct.
                self.cache = fmt::unwrap!(heapless::Vec::from_slice(into_cache));

                break;
            }

            bytes_read += chunk.len();

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

        self.pos += bytes_read as u16;

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
        assert_eq!(r.skip_ahead_bytes(63).await, Ok(()), "63 bytes");

        let mut r = ChunkReader::new(EepromFile::new("dumps/eeprom/akd.hex"), 0, 32);

        // Off by one errors are always fun
        assert_eq!(
            r.skip_ahead_bytes(64).await,
            Err(Error::Eeprom(EepromError::SectionOverrun)),
            "64 bytes"
        );

        let mut r = ChunkReader::new(EepromFile::new("dumps/eeprom/akd.hex"), 0, 32);

        // 65 is one byte off the end
        assert_eq!(
            r.skip_ahead_bytes(65).await,
            Err(Error::Eeprom(EepromError::SectionOverrun)),
            "65 bytes"
        );

        let mut r = ChunkReader::new(EepromFile::new("dumps/eeprom/akd.hex"), 0, 32);

        // Madness
        assert_eq!(
            r.skip_ahead_bytes(10000).await,
            Err(Error::Eeprom(EepromError::SectionOverrun)),
            "10000 bytes"
        );
    }
}
