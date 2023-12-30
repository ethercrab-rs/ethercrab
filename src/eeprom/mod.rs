use core::ops::Deref;

use self::{device_reader::SII_FIRST_CATEGORY_START, types::CategoryType};
use crate::{
    error::{EepromError, Error},
    fmt,
};
use embedded_io_async::{ErrorType, Read, ReadExactError, Seek, SeekFrom};

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
    /// Current byte position in the entire address space.
    pos: u16,
    // /// Chunk size in bytes.
    // len: u16,
    /// The last byte address we're allowed to access.
    end: u16,
    cache: heapless::Vec<u8, 8>,
}

impl<P> ChunkReader<P> {
    pub fn new(reader: P, start_word: u16, len_words: u16) -> Self {
        Self {
            reader,
            pos: start_word * 2,
            // len: len_bytes,
            end: start_word * 2 + len_words * 2,
            cache: heapless::Vec::new(),
        }
    }

    /// Skip N bytes (NOT words) ahead of the current position.
    pub fn skip_ahead_bytes(&mut self, skip: u16) -> Result<(), Error> {
        fmt::trace!(
            "Skip EEPROM. Pos {:#06x}, skip by {} to {:#06x}, end {:#06x}",
            self.pos,
            skip,
            self.pos + skip,
            self.end
        );

        if self.pos + skip >= self.end {
            return Err(Error::Eeprom(EepromError::SectionOverrun));
        }

        // Move forward in the cache and discard bytes in the cache before the position we're
        // skipping too.
        if usize::from(skip) <= self.cache.len() {
            self.pos += skip;

            let (_discard, rest) = self.cache.split_at(usize::from(skip));

            self.cache = fmt::unwrap!(heapless::Vec::from_slice(rest));
        }
        // If we've skipped past the existing cache, read a word-aligned chunk and discard the first
        // byte if the skip length is odd to re-align everything to byte boundaries.
        else {
            todo!()
        }

        Ok(())
    }
}

impl<P> Read for ChunkReader<P>
where
    P: EepromDataProvider,
{
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        fmt::trace!("Read chunk");

        let DELETEME = buf.len();

        let max_read = usize::from(self.end - self.pos);

        let mut bytes_read = 0;

        if max_read == 0 {
            return Ok(0);
        }

        let buf_len = buf.len();

        // Clamp buffer to whatever's left to read in this chunk
        let buf = &mut buf[0..buf_len.min(max_read)];

        // Split off existing cache buffer and write into buf
        let (cached, rest) = self.cache.split_at(buf.len().min(self.cache.len()));

        let (start, mut buf) = buf.split_at_mut(cached.len());

        start.copy_from_slice(cached);

        self.pos += cached.len() as u16;
        bytes_read += cached.len();

        // Re-store any remaining cached data ready for the next read.
        //
        // This should never panic as the source data is the same type, so it can be at most exactly
        // the same length, or shorter.
        self.cache = fmt::unwrap!(heapless::Vec::from_slice(rest));

        // If there is more to read, read chunks from provider until buffer is full
        while buf.len() > 0 {
            fmt::trace!(
                "----> Loop, buf len {} from pos {} {:02x}",
                buf.len(),
                self.pos,
                self.pos
            );
            let res = self.reader.read_chunk(self.pos / 2).await?;

            self.pos += res.len() as u16;

            let chunk = res.deref();

            fmt::trace!(
                "----> Read loop pos {} {:02x}, chunk {:02x?}",
                self.pos,
                self.pos,
                chunk
            );

            // Buffer is full. We're done.
            if buf.len() < chunk.len() {
                let (chunk, into_cache) = chunk.split_at(buf.len());

                bytes_read += chunk.len();

                buf.copy_from_slice(chunk);

                // Unwrap: logic bug! The returned chunk must be either 4 or 8 bytes, and we have a
                // vec of 8 bytes in length, so this should not fail if everything is correct.
                self.cache = fmt::unwrap!(heapless::Vec::from_slice(into_cache));

                fmt::trace!("----> Chunk is done. Cache has: {:02x?}", self.cache);

                break;
            }

            bytes_read += chunk.len();

            // Buffer is not full. Write another chunk into it.
            let (start, rest) = buf.split_at_mut(chunk.len());

            start.copy_from_slice(chunk);

            fmt::trace!("--> Buf for next iter {}", rest.len());

            buf = rest;
        }

        // Store any remaining chunk in buffer for next read.

        // ---

        fmt::trace!(
            "--> Done. Read {} of requested {}, pos is now {:#06x}",
            bytes_read,
            DELETEME,
            self.pos
        );

        Ok(bytes_read)

        // // We've read the entire category. We're finished now.
        // if self.pos >= self.len {
        //     return Ok(0);
        // }

        // // let buf_end = self.pos + buf.len() as u16;

        // let clamped_buf_len = buf.len().min(usize::from(self.len - self.pos));

        // dbg!(self.pos, self.len, buf.len(), clamped_buf_len);

        // let mut buf = &mut buf[..clamped_buf_len];

        // self.reader.read_exact(&mut buf).await?;

        // self.pos += clamped_buf_len as u16;

        // Ok(clamped_buf_len)
    }
}

impl<P> ErrorType for ChunkReader<P> {
    type Error = Error;
}

// // TODO: Delete all/as much of this as possible
// impl<P> ChunkReader<P>
// where
//     P: EepromDataProvider,
// {
//     pub fn new(reader: P, len: u16) -> Self {
//         Self {
//             reader,
//             pos: 0,
//             len,
//         }
//     }

//     /// Skip a given number of addresses (note: not bytes).
//     pub async fn skip(&mut self, skip: u16) -> Result<(), Error> {
//         // TODO: Optimise by calculating new skip address instead of just iterating through chunks
//         for _ in 0..skip {
//             self.next().await?;
//         }

//         Ok(())
//     }

//     pub async fn next(&mut self) -> Result<Option<u8>, Error> {
//         let mut buf = [0u8; 1];

//         // We've read the entire category. We're finished now.
//         if self.pos >= self.len {
//             return Ok(None);
//         }

//         self.reader.read_exact(&mut buf).await?;

//         self.pos += 1;

//         Ok(Some(buf[0]))
//     }

//     /// Try reading the next chunk in the current section.
//     pub async fn try_next(&mut self) -> Result<u8, Error> {
//         match self.next().await? {
//             Some(value) => Ok(value),
//             None => Err(Error::Eeprom(EepromError::SectionOverrun)),
//         }
//     }

//     /// Attempt to read exactly `N` bytes. If not enough data could be read, this method returns an
//     /// error.
//     pub async fn take_vec_exact<const N: usize>(&mut self) -> Result<heapless::Vec<u8, N>, Error> {
//         self.take_vec()
//             .await?
//             .ok_or(Error::Eeprom(EepromError::SectionUnderrun))
//     }

//     /// Read up to `N` bytes. If not enough data could be read, this method will return `Ok(None)`.
//     pub async fn take_vec<const N: usize>(
//         &mut self,
//     ) -> Result<Option<heapless::Vec<u8, N>>, Error> {
//         self.take_vec_len(N).await
//     }

//     /// Try to take `len` bytes, returning an error if the buffer length `N` is too small.
//     ///
//     /// If not enough data could be read, this method returns an error.
//     pub async fn take_vec_len_exact<const N: usize>(
//         &mut self,
//         len: usize,
//     ) -> Result<heapless::Vec<u8, N>, Error> {
//         self.take_vec_len(len)
//             .await?
//             .ok_or(Error::Eeprom(EepromError::SectionUnderrun))
//     }

//     /// Try to take `len` bytes, returning an error if the buffer length `N` is too small.
//     ///
//     /// If not enough data can be read to fill the buffer, this method will return `Ok(None)`.
//     async fn take_vec_len<const N: usize>(
//         &mut self,
//         len: usize,
//     ) -> Result<Option<heapless::Vec<u8, N>>, Error> {
//         let mut buf = heapless::Vec::new();

//         let mut count = 0;

//         loop {
//             // We've collected the requested number of bytes
//             if count >= len {
//                 break Ok(Some(buf));
//             }

//             // If buffer is full, we'd end up with truncated data, so error out.
//             if buf.is_full() {
//                 fmt::error!("take_vec_len output buffer is full");

//                 break Err(Error::Eeprom(EepromError::SectionOverrun));
//             }

//             if let Some(byte) = self.next().await? {
//                 // SAFETY: We check for buffer space using is_full above
//                 unsafe { buf.push_unchecked(byte) };

//                 count += 1;
//             } else {
//                 // Not enough data to fill the buffer
//                 break Ok(None);
//             }
//         }
//     }
// }
