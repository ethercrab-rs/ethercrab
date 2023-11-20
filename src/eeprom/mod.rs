use crate::{
    eeprom::types::CategoryType,
    error::{EepromError, Error},
    fmt,
};

pub mod reader;
pub mod types;

#[cfg(feature = "std")]
pub mod file_reader;

/// A data source for EEPROM reads.
pub trait EepromDataProvider {
    type Handle: EepromBlock;

    async fn category(&self, category: CategoryType) -> Result<Option<Self::Handle>, Error>;

    fn address(&self, address: u16, len_bytes: u16) -> Self::Handle;
}

/// A reader for a single category of a device EEPROM.
pub trait EepromBlock {
    /// Read the next byte from the EEPROM.
    ///
    /// Internally, this method reads the EEPROM in chunks of 4 or 8 bytes (depending on the slave).
    async fn next(&mut self) -> Result<Option<u8>, Error>;

    /// Skip a given number of addresses (note: not bytes).
    async fn skip(&mut self, skip: u16) -> Result<(), Error> {
        // TODO: Optimise by calculating new skip address instead of just iterating through chunks
        for _ in 0..skip {
            self.next().await?;
        }

        Ok(())
    }

    /// Try reading the next chunk in the current section.
    async fn try_next(&mut self) -> Result<u8, Error> {
        match self.next().await? {
            Some(value) => Ok(value),
            None => Err(Error::Eeprom(EepromError::SectionOverrun)),
        }
    }

    /// Attempt to read exactly `N` bytes. If not enough data could be read, this method returns an
    /// error.
    async fn take_vec_exact<const N: usize>(&mut self) -> Result<heapless::Vec<u8, N>, Error> {
        self.take_vec()
            .await?
            .ok_or(Error::Eeprom(EepromError::SectionUnderrun))
    }

    /// Read up to `N` bytes. If not enough data could be read, this method will return `Ok(None)`.
    async fn take_vec<const N: usize>(&mut self) -> Result<Option<heapless::Vec<u8, N>>, Error> {
        self.take_vec_len(N).await
    }

    /// Try to take `len` bytes, returning an error if the buffer length `N` is too small.
    ///
    /// If not enough data could be read, this method returns an error.
    async fn take_vec_len_exact<const N: usize>(
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
