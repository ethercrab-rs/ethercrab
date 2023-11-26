use crate::{
    eeprom::types::CategoryType,
    error::{EepromError, Error},
    fmt,
    slave::slave_client::SlaveClient,
};

pub mod reader;
pub mod types;

// #[cfg(feature = "std")]
// pub mod file_reader;

/// A data source for EEPROM reads.
///
/// This provides a method `reader` which creates handles into the underlying storage.
pub trait EepromDataProvider {
    /// A reader instance that returns bytes from the underlying data source.
    type Provider: embedded_io_async::Read + embedded_io_async::Seek;

    /// Get an instance of a reader.
    fn reader(&self) -> Self::Provider;
}

impl embedded_io_async::Error for Error {
    fn kind(&self) -> embedded_io_async::ErrorKind {
        // TODO: match()?
        embedded_io_async::ErrorKind::Other
    }
}

pub struct ChunkReader<P> {
    reader: P,
    /// Max number of bytes we're allowed to read
    len: u16,
    /// Current number of bytes we've read
    byte_count: usize,
}

impl<P> ChunkReader<P>
where
    P: embedded_io_async::Read,
{
    pub fn new(reader: P, len_bytes: u16) -> Self {
        Self {
            reader,
            len: len_bytes,
            byte_count: 0,
        }
    }
}

// impl<B> CategoryWrapper<B>
// where
//     B: embedded_io_async::Read,
// {
//     pub fn new(block: B) -> Self {
//         Self { block }
//     }

//     /// Skip a given number of addresses (note: not bytes).
//     pub async fn skip(&mut self, skip: u16) -> Result<(), Error> {
//         // TODO: Optimise by calculating new skip address instead of just iterating through chunks
//         for _ in 0..skip {
//             self.block.next().await?;
//         }

//         Ok(())
//     }

//     pub async fn next(&mut self) -> Result<Option<u8>, Error> {
//         self.block.next().await
//     }

//     /// Try reading the next chunk in the current section.
//     pub async fn try_next(&mut self) -> Result<u8, Error> {
//         match self.block.next().await? {
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
