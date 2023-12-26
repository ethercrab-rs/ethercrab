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

/// A data source for EEPROM reads.
///
/// This provides a method `reader` which creates handles into the underlying storage.
pub trait EepromDataProvider {
    /// A reader instance that returns bytes from the underlying data source.
    type Provider: Read + Seek + ErrorType<Error = Error> + core::fmt::Debug;

    /// Get an instance of a reader.
    fn reader(&self) -> Self::Provider;

    /// Find the length in bytes of the EEPROM.
    // Internal only so I don't mind
    #[allow(async_fn_in_trait)]
    async fn len(&self) -> Result<u16, Error> {
        let mut reader = self.reader();

        reader
            .seek(SeekFrom::Start(SII_FIRST_CATEGORY_START.into()))
            .await?;

        let mut len_bytes = SII_FIRST_CATEGORY_START * 2;

        loop {
            let mut category_type = [0u8; 2];
            let mut len_words = [0u8; 2];

            reader.read_exact(&mut category_type).await?;
            reader.read_exact(&mut len_words).await?;

            // Add header
            len_bytes += 4;

            let category_type = CategoryType::from(u16::from_le_bytes(category_type));
            let len_words = u16::from_le_bytes(len_words);

            if let CategoryType::End = category_type {
                break Ok(len_bytes);
            }

            // Now add category data length
            len_bytes += len_words * 2;

            // Next category starts after the current category's data. Seek takes a WORD address
            reader.seek(SeekFrom::Current(len_words.into())).await?;
        }
    }
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

#[derive(Debug)]
pub struct ChunkReader<P> {
    reader: P,
    /// Max number of bytes we're allowed to read
    len: u16,
    /// Current number of bytes we've read
    byte_count: usize,
}

impl<P> ChunkReader<P>
where
    P: Read + ErrorType<Error = Error>,
{
    pub fn new(reader: P, len_bytes: u16) -> Self {
        Self {
            reader,
            len: len_bytes,
            byte_count: 0,
        }
    }

    /// Skip a given number of addresses (note: not bytes).
    pub async fn skip(&mut self, skip: u16) -> Result<(), Error> {
        // TODO: Optimise by calculating new skip address instead of just iterating through chunks
        for _ in 0..skip {
            self.next().await?;
        }

        Ok(())
    }

    pub async fn next(&mut self) -> Result<Option<u8>, Error> {
        let mut buf = [0u8; 1];

        // We've read the entire category. We're finished now.
        if self.byte_count >= usize::from(self.len) {
            return Ok(None);
        }

        self.reader.read_exact(&mut buf).await?;

        self.byte_count += 1;

        Ok(Some(buf[0]))
    }

    /// Try reading the next chunk in the current section.
    pub async fn try_next(&mut self) -> Result<u8, Error> {
        match self.next().await? {
            Some(value) => Ok(value),
            None => Err(Error::Eeprom(EepromError::SectionOverrun)),
        }
    }

    /// Attempt to read exactly `N` bytes. If not enough data could be read, this method returns an
    /// error.
    pub async fn take_vec_exact<const N: usize>(&mut self) -> Result<heapless::Vec<u8, N>, Error> {
        self.take_vec()
            .await?
            .ok_or(Error::Eeprom(EepromError::SectionUnderrun))
    }

    /// Read up to `N` bytes. If not enough data could be read, this method will return `Ok(None)`.
    pub async fn take_vec<const N: usize>(
        &mut self,
    ) -> Result<Option<heapless::Vec<u8, N>>, Error> {
        self.take_vec_len(N).await
    }

    /// Try to take `len` bytes, returning an error if the buffer length `N` is too small.
    ///
    /// If not enough data could be read, this method returns an error.
    pub async fn take_vec_len_exact<const N: usize>(
        &mut self,
        len: usize,
    ) -> Result<heapless::Vec<u8, N>, Error> {
        self.take_vec_len(len)
            .await?
            .ok_or(Error::Eeprom(EepromError::SectionUnderrun))
    }

    /// Try to take `len` bytes, returning an error if the buffer length `N` is too small.
    ///
    /// If not enough data can be read to fill the buffer, this method will return `Ok(None)`.
    async fn take_vec_len<const N: usize>(
        &mut self,
        len: usize,
    ) -> Result<Option<heapless::Vec<u8, N>>, Error> {
        let mut buf = heapless::Vec::new();

        let mut count = 0;

        loop {
            // We've collected the requested number of bytes
            if count >= len {
                break Ok(Some(buf));
            }

            // If buffer is full, we'd end up with truncated data, so error out.
            if buf.is_full() {
                fmt::error!("take_vec_len output buffer is full");

                break Err(Error::Eeprom(EepromError::SectionOverrun));
            }

            if let Some(byte) = self.next().await? {
                // SAFETY: We check for buffer space using is_full above
                unsafe { buf.push_unchecked(byte) };

                count += 1;
            } else {
                // Not enough data to fill the buffer
                break Ok(None);
            }
        }
    }
}
