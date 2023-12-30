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

// pub trait ChunkReaderTraitLol {
//     async fn read_chunk(
//         &mut self,
//         start_word: u16,
//     ) -> Result<impl core::ops::Deref<Target = [u8]>, Error>;

//     fn seek_to_word(&mut self, word: u16);
// }

/// A data source for EEPROM reads.
///
/// This provides a method `reader` which creates handles into the underlying storage.
pub trait EepromDataProvider: Clone {
    async fn read_chunk(&mut self, start_word: u16) -> Result<impl Deref<Target = [u8]>, Error>;

    // /// Find the length in bytes of the EEPROM.
    // // Internal only so I don't mind
    // #[allow(async_fn_in_trait)]
    // async fn len(&self) -> Result<u16, Error> {
    //     let mut reader = self.reader();

    //     reader
    //         .seek(SeekFrom::Start(SII_FIRST_CATEGORY_START.into()))
    //         .await?;

    //     let mut len_bytes = SII_FIRST_CATEGORY_START * 2;

    //     loop {
    //         let mut category_type = [0u8; 2];
    //         let mut len_words = [0u8; 2];

    //         reader.read_exact(&mut category_type).await?;
    //         reader.read_exact(&mut len_words).await?;

    //         // Add header
    //         len_bytes += 4;

    //         let category_type = CategoryType::from(u16::from_le_bytes(category_type));
    //         let len_words = u16::from_le_bytes(len_words);

    //         if let CategoryType::End = category_type {
    //             break Ok(len_bytes);
    //         }

    //         // Now add category data length
    //         len_bytes += len_words * 2;

    //         // Next category starts after the current category's data. Seek takes a WORD address
    //         reader.seek(SeekFrom::Current(len_words.into())).await?;
    //     }
    // }
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
#[derive(Debug)]
pub struct ChunkReader<P> {
    reader: P,
    /// Current logical byte position in the entire address space.
    ///
    /// This is the last byte that was returned by the reader, and should be used as a base for skip
    /// offsets.
    pos: u16,
    /// The last byte address we're allowed to access.
    end: u16,
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

        // ---

        // todo!();

        // // Move forward in the cache and discard bytes in the cache before the position we're
        // // skipping too.
        // if usize::from(skip) <= self.cache.len() {
        //     self.cache = fmt::unwrap!(heapless::Vec::from_slice(&self.cache[usize::from(skip)..]));
        // }
        // // If we've skipped past the existing cache, read a word-aligned chunk and discard the first
        // // byte if the skip length is odd to re-align everything to byte boundaries.
        // else {
        //     self.pos = self.read_pointer;

        //     let chunk = self.reader.read_chunk(self.pos / 2).await?;

        //     let chunk = chunk.deref();

        //     // Word address is rounded down so we discard the first byte in the read chunk to
        //     // re-align to the byte position if it's odd. If it's even we don't need to do anything.
        //     let discard_len = usize::from(self.pos % 2);

        //     let new_cache = &chunk[discard_len..];

        //     fmt::trace!(
        //         "Discarding {} bytes to realign word read to byte boundaries. Pos is {:#06x?}, chunk is {:02x?}, new cache is {:02x?}",
        //         discard_len,
        //         self.pos,
        //         chunk,
        //         new_cache
        //     );

        //     self.cache = fmt::unwrap!(heapless::Vec::from_slice(new_cache));

        //     // Next read will be after this current chunk we've just put in the cache
        //     self.pos += chunk.len() as u16;
        //     self.read_pointer = self.pos;
        // }

        Ok(())
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
