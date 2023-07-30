//! Traits and impls used to read/write data to/from the wire.

use crate::{error::VisibleStringError, LEN_MASK};
use core::array::TryFromSliceError;

pub trait PduRead: Sized {
    const LEN: u16;

    #[cfg(feature = "defmt")]
    type Error: defmt::Format;

    #[cfg(not(feature = "defmt"))]
    type Error: code::fmt::Debug;

    fn len() -> u16 {
        Self::LEN & LEN_MASK
    }

    fn try_from_slice(slice: &[u8]) -> Result<Self, Self::Error>;
}

pub trait PduData: PduRead {
    fn as_slice(&self) -> &[u8];
}

macro_rules! impl_pdudata {
    ($ty:ty) => {
        impl PduRead for $ty {
            const LEN: u16 = Self::BITS as u16 / 8;
            type Error = TryFromSliceError;

            fn try_from_slice(slice: &[u8]) -> Result<Self, Self::Error> {
                Ok(Self::from_le_bytes(slice.try_into()?))
            }
        }

        impl PduData for $ty {
            fn as_slice(&self) -> &[u8] {
                safe_transmute::to_bytes::transmute_one_to_bytes(self)
            }
        }
    };
}

impl_pdudata!(u8);
impl_pdudata!(u16);
impl_pdudata!(u32);
impl_pdudata!(u64);
impl_pdudata!(i8);
impl_pdudata!(i16);
impl_pdudata!(i32);
impl_pdudata!(i64);

impl<const N: usize> PduData for [u8; N] {
    fn as_slice(&self) -> &[u8] {
        self
    }
}

impl PduRead for () {
    const LEN: u16 = 0;

    type Error = TryFromSliceError;

    fn try_from_slice(_slice: &[u8]) -> Result<Self, Self::Error> {
        Ok(())
    }
}

impl PduData for () {
    fn as_slice(&self) -> &[u8] {
        &[]
    }
}

impl<const N: usize, T> PduRead for [T; N]
where
    T: PduRead,
{
    const LEN: u16 = T::LEN * N as u16;

    type Error = ();

    fn try_from_slice(slice: &[u8]) -> Result<Self, Self::Error> {
        let chunks = slice.chunks_exact(usize::from(T::LEN));

        let mut res = heapless::Vec::<T, N>::new();

        for chunk in chunks {
            res.push(T::try_from_slice(chunk).map_err(|_| ())?)
                .map_err(|_| ())?;
        }

        res.into_array().map_err(|_| ())
    }
}

impl<const N: usize> PduRead for heapless::String<N> {
    const LEN: u16 = N as u16;

    type Error = VisibleStringError;

    fn try_from_slice(slice: &[u8]) -> Result<Self, Self::Error> {
        let mut out = heapless::String::new();

        out.push_str(core::str::from_utf8(slice).map_err(VisibleStringError::Decode)?)
            .map_err(|_| VisibleStringError::TooLong)?;

        Ok(out)
    }
}

/// A "Visible String" representation. Characters are specified to be within the ASCII range.
impl<const N: usize> PduData for heapless::String<N> {
    fn as_slice(&self) -> &[u8] {
        self.as_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg_attr(miri, ignore)]
    fn fuzz_pdu_data_array() {
        heckcheck::check(|data: [u8; 8]| {
            let parsed = <[[u8; 2]; 4]>::try_from_slice(&data);

            let expected = [
                [data[0], data[1]],
                [data[2], data[3]],
                [data[4], data[5]],
                [data[6], data[7]],
            ];

            assert_eq!(parsed, Ok(expected));

            Ok(())
        });
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn fuzz_pdu_data_roundtrip() {
        heckcheck::check(|data: [u8; 8]| {
            // u8
            {
                let data = data[0];
                let sl = data.as_slice();
                let decoded = u8::try_from_slice(sl).expect("u8 from slice");
                assert_eq!(decoded, data);
            }

            // u16
            {
                let data = u16::from_le_bytes(data[0..2].try_into().unwrap());
                let sl = data.as_slice();
                let decoded = u16::try_from_slice(sl).expect("u16 from slice");
                assert_eq!(decoded, data);
            }

            // u32
            {
                let data = u32::from_le_bytes(data[0..4].try_into().unwrap());
                let sl = data.as_slice();
                let decoded = u32::try_from_slice(sl).expect("u32 from slice");
                assert_eq!(decoded, data);
            }

            // u64
            {
                let data = u64::from_le_bytes(data[0..8].try_into().unwrap());
                let sl = data.as_slice();
                let decoded = u64::try_from_slice(sl).expect("u64 from slice");
                assert_eq!(decoded, data);
            }

            // i8
            {
                let data = data[0] as i8;
                let sl = data.as_slice();
                let decoded = i8::try_from_slice(sl).expect("i8 from slice");
                assert_eq!(decoded, data);
            }

            // i16
            {
                let data = i16::from_le_bytes(data[0..2].try_into().unwrap());
                let sl = data.as_slice();
                let decoded = i16::try_from_slice(sl).expect("i16 from slice");
                assert_eq!(decoded, data);
            }

            // i32
            {
                let data = i32::from_le_bytes(data[0..4].try_into().unwrap());
                let sl = data.as_slice();
                let decoded = i32::try_from_slice(sl).expect("i32 from slice");
                assert_eq!(decoded, data);
            }

            // i64
            {
                let data = i64::from_le_bytes(data[0..8].try_into().unwrap());
                let sl = data.as_slice();
                let decoded = i64::try_from_slice(sl).expect("i64 from slice");
                assert_eq!(decoded, data);
            }

            Ok(())
        });
    }
}
