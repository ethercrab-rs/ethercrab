use core::ops::Deref;

use crate::{
    error::{EepromError, Error},
    fmt,
};
use embedded_io_async::{ErrorType, ReadExactError};

pub mod device_provider;
pub mod types;

#[cfg(feature = "std")]
pub mod file_provider;

pub const STATION_ALIAS_POSITION: core::ops::Range<usize> = 8..10;
pub const CHECKSUM_POSITION: core::ops::Range<usize> = 14..16;

const ECAT_CRC_ALGORITHM: crc::Algorithm<u8> = crc::Algorithm {
    width: 8,
    poly: 0x07,
    init: 0xff,
    refin: false,
    refout: false,
    xorout: 0x00,
    check: 0x80,
    residue: 0x00,
};

pub const STATION_ALIAS_CRC: crc::Crc<u8> = crc::Crc::<u8>::new(&ECAT_CRC_ALGORITHM);

/// A data source for EEPROM reads.
pub trait EepromDataProvider: Clone {
    /// Read a chunk of either 4 or 8 bytes from the backing store.
    async fn read_chunk(&mut self, start_word: u16) -> Result<impl Deref<Target = [u8]>, Error>;

    /// Write two bytes into the SubDevice EEPROM at the given address
    async fn write_word(&mut self, start_word: u16, data: [u8; 2]) -> Result<(), Error>;

    /// Attempt to clear any errors in the EEPROM source.
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

/// An abstraction over a provider of EEPROM bytes that only allows a certain range to be read or
/// written.
///
/// The provider `P` should be as simple as possible, returning chunks of data either 4 or 8 bytes
/// long or writing a single word (2 bytes). Other lengths are not tested as the EtherCAT
/// specification requires/supports only 4 or 8 byte SII reads.
#[derive(Debug)]
pub struct EepromRange<P> {
    reader: P,

    /// Current logical byte position in the entire address space.
    ///
    /// This is the last byte that was returned to the caller by the reader, and should be used as a
    /// base for skip offsets.
    byte_pos: u16,

    /// The last byte address we're allowed to access.
    end: u16,
}

impl<P> EepromRange<P>
where
    P: EepromDataProvider,
{
    /// Create a new `ChunkReader`.
    pub fn new(reader: P, start_word: u16, len_words: u16) -> Self {
        Self {
            reader,
            byte_pos: start_word * 2,
            end: start_word * 2 + len_words * 2,
        }
    }

    /// Skip N bytes (NOT words) ahead of the current position.
    pub fn skip_ahead_bytes(&mut self, skip: u16) -> Result<(), EepromError> {
        fmt::trace!(
            "Skip EEPROM from pos {:#06x} by {} bytes to {:#06x}",
            self.byte_pos,
            skip,
            self.byte_pos + skip,
        );

        if self.byte_pos + skip >= self.end {
            return Err(EepromError::SectionOverrun);
        }

        self.byte_pos += skip;

        Ok(())
    }

    /// Read a single byte.
    pub async fn read_byte(&mut self) -> Result<u8, Error> {
        self.reader.clear_errors().await?;

        let res = self.reader.read_chunk(self.byte_pos / 2).await?;

        // pos is in bytes, but we're reading words. If the current pos is odd, we must skip the
        // first byte of the returned word.
        let skip = usize::from(self.byte_pos % 2);

        // Advance by one byte
        self.byte_pos += 1;

        res.get(skip).copied().ok_or(Error::Internal)
    }

    #[allow(unused)]
    pub(crate) fn into_inner(self) -> P {
        self.reader
    }
}

impl<P> embedded_io_async::Read for EepromRange<P>
where
    P: EepromDataProvider,
{
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        fmt::trace!("Read EEPROM chunk from byte {:#06x}", self.byte_pos);

        let requested_read_len = buf.len();

        let max_read = usize::from(self.end.saturating_sub(self.byte_pos));

        let mut bytes_read = 0;

        // The read pointer has reached the end of the chunk
        if max_read == 0 {
            return Ok(0);
        }

        // We can't read past the end of the chunk, so clamp the buffer's length to the remaining
        // part of the chunk if necessary.
        let mut buf = buf
            .get_mut(0..requested_read_len.min(max_read))
            .ok_or(Error::Internal)?;

        self.reader.clear_errors().await?;

        while !buf.is_empty() {
            let res = self.reader.read_chunk(self.byte_pos / 2).await?;

            let chunk = &*res;

            // If position is odd, we must skip the first received byte as the reader operates on
            // WORD addresses.
            let skip = usize::from(self.byte_pos % 2);

            // Fix any odd addressing offsets
            let chunk = chunk.get(skip..).ok_or(Error::Internal)?;

            // Buffer is full after reading this chunk into it. We're done.
            if buf.len() < chunk.len() {
                let (chunk, _rest) = chunk.split_at(buf.len());

                bytes_read += chunk.len();
                self.byte_pos += chunk.len() as u16;

                buf.copy_from_slice(chunk);

                break;
            }

            bytes_read += chunk.len();
            self.byte_pos += chunk.len() as u16;

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
            self.byte_pos
        );

        Ok(bytes_read)
    }
}

impl<P> embedded_io_async::Write for EepromRange<P>
where
    P: EepromDataProvider,
{
    async fn write(&mut self, mut buf: &[u8]) -> Result<usize, Self::Error> {
        fmt::trace!(
            "Write EEPROM word from byte position {:#06x}",
            self.byte_pos
        );

        let len = buf.len();
        let mut written = 0;

        loop {
            // The pointer has reached the end of the chunk
            if self.end.saturating_sub(self.byte_pos) == 0 {
                break;
            }

            let Some((word, rest)) = buf
                .split_first_chunk::<2>()
                .map(|(word, rest)| (*word, rest))
                .or_else(|| {
                    // Handle cases where the buffer length is odd. We'll pad with zeros.
                    buf.split_first()
                        .map(|(first, rest)| ([*first, 0x00], rest))
                })
            else {
                break;
            };

            self.reader.write_word(self.byte_pos / 2, word).await?;

            written += word.len();
            self.byte_pos += word.len() as u16;

            buf = rest;
        }

        fmt::trace!(
            "--> Done. Wrote {} of requested {} B, position is now {:#06x}",
            written,
            len,
            self.byte_pos
        );

        Ok(written)
    }
}

impl<P> ErrorType for EepromRange<P> {
    type Error = Error;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eeprom::file_provider::EepromFile;
    use embedded_io_async::{Read, Write};

    #[tokio::test]
    async fn skip_past_end() {
        crate::test_logger();

        let mut r = EepromRange::new(
            EepromFile::new(include_bytes!("../../dumps/eeprom/akd.hex")),
            0,
            32,
        );

        // Current position is zero, so 32 words = 64 bytes = ok
        assert_eq!(r.skip_ahead_bytes(63), Ok(()), "63 bytes");

        let mut r = EepromRange::new(
            EepromFile::new(include_bytes!("../../dumps/eeprom/akd.hex")),
            0,
            32,
        );

        // Off by one errors are always fun
        assert_eq!(
            r.skip_ahead_bytes(64),
            Err(EepromError::SectionOverrun),
            "64 bytes"
        );

        let mut r = EepromRange::new(
            EepromFile::new(include_bytes!("../../dumps/eeprom/akd.hex")),
            0,
            32,
        );

        // 65 is one byte off the end
        assert_eq!(
            r.skip_ahead_bytes(65),
            Err(EepromError::SectionOverrun),
            "65 bytes"
        );

        let mut r = EepromRange::new(
            EepromFile::new(include_bytes!("../../dumps/eeprom/akd.hex")),
            0,
            32,
        );

        // Madness
        assert_eq!(
            r.skip_ahead_bytes(10000),
            Err(EepromError::SectionOverrun),
            "10000 bytes"
        );
    }

    #[tokio::test]
    async fn read_single_bytes() {
        crate::test_logger();

        let mut r = EepromRange::new(
            EepromFile::new(include_bytes!("../../dumps/eeprom/el2828.hex")),
            0,
            32,
        );

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

    #[tokio::test]
    async fn read_checksum_el2828() {
        let mut r = EepromRange::new(
            EepromFile::new(include_bytes!("../../dumps/eeprom/el2828.hex")),
            // Start at beginning of EEPROM
            0,
            // 8 words, 16 bytes
            8,
        );

        // 8 words or 16 bytes
        let mut all = [0u8; 16];

        r.read_exact(&mut all).await.expect("Read");

        let (rest, checksum) = all.split_last_chunk::<2>().unwrap();

        assert_eq!(rest.len(), 14);

        // The lower byte of the last word is the checksum of the previous 14 bytes
        let checksum = u16::from_le_bytes(*checksum);

        let expected_checksum = 0x00e2u16;

        assert_eq!(checksum, expected_checksum);

        const ECAT_CRC: crc::Algorithm<u8> = crc::Algorithm {
            width: 8,
            poly: 0x07,
            init: 0xff,
            refin: false,
            refout: false,
            xorout: 0x00,
            check: 0x80,
            residue: 0x00,
        };

        const EEPROM_CRC: crc::Crc<u8> = crc::Crc::<u8>::new(&ECAT_CRC);

        let cs = u16::from(EEPROM_CRC.checksum(rest));

        assert_eq!(
            cs, expected_checksum,
            "{:#04x} {:#04x}",
            cs, expected_checksum
        );
    }

    #[tokio::test]
    async fn read_checksum_akd() {
        let mut r = EepromRange::new(
            EepromFile::new(include_bytes!("../../dumps/eeprom/akd.hex")),
            // Start at beginning of EEPROM
            0,
            // 8 words, 16 bytes
            8,
        );

        // 8 words or 16 bytes
        let mut all = [0u8; 16];

        r.read_exact(&mut all).await.expect("Read");

        let (rest, checksum) = all.split_last_chunk::<2>().unwrap();

        assert_eq!(rest.len(), 14);

        // The lower byte of the last word is the checksum of the previous 14 bytes
        let checksum = u16::from_le_bytes(*checksum);

        let expected_checksum = 0x0010u16;

        assert_eq!(checksum, expected_checksum);

        const ECAT_CRC: crc::Algorithm<u8> = crc::Algorithm {
            width: 8,
            poly: 0x07,
            init: 0xff,
            refin: false,
            refout: false,
            xorout: 0x00,
            check: 0x80,
            residue: 0x00,
        };

        const EEPROM_CRC: crc::Crc<u8> = crc::Crc::<u8>::new(&ECAT_CRC);

        let cs = u16::from(EEPROM_CRC.checksum(rest));

        assert_eq!(
            cs, expected_checksum,
            "{:#04x} {:#04x}",
            cs, expected_checksum
        );
    }

    #[tokio::test]
    async fn write_station_alias() {
        let mut r = EepromRange::new(
            EepromFile::new(include_bytes!("../../dumps/eeprom/akd.hex")),
            // Start at beginning of EEPROM
            0,
            // 8 words, 16 bytes
            8,
        );

        // Read first block and checksum
        let mut all = [0u8; 16];

        r.read_exact(&mut all).await.expect("Read");

        let existing_alias = u16::from_le_bytes(all[STATION_ALIAS_POSITION].try_into().unwrap());

        let new_alias = 0xabcd_u16;

        assert_eq!(existing_alias, 0x0000);
        assert_ne!(new_alias, existing_alias);

        all[STATION_ALIAS_POSITION].copy_from_slice(&new_alias.to_le_bytes());

        // Don't checksum the checksum
        let checksum = u16::from(STATION_ALIAS_CRC.checksum(&all[0..CHECKSUM_POSITION.start]));

        // Update checksum ready to write back into EEPROM
        all[CHECKSUM_POSITION].copy_from_slice(&checksum.to_le_bytes());

        let expected = [
            0x09, 0x00, 0x00, 0x08, 0x00, 0x00, 0x00, 0x00, // Etc
            0xcd, 0xab, // Station alias, LE
            0x00, 0x00, 0x00, 0x00, // Reserved bytes
            0x04, 0x00, // Checksum
        ];

        // Check what we're going to write is correct
        assert_eq!(all, expected);

        // Make a new instance to reset all the buffer pointers to the beginning
        let mut w = EepromRange::new(
            EepromFile::new(include_bytes!("../../dumps/eeprom/akd.hex")),
            // Start at beginning of EEPROM
            0,
            // 8 words, 16 bytes
            8,
        );

        w.write_all(&all).await.expect("Write failed");

        // Check what we wrote is correct
        assert_eq!(w.into_inner().write_cache[0..16], expected);
    }
}
