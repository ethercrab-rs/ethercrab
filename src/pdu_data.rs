//! Traits and impls used to read/write data to/from the wire.

use crate::{error::VisibleStringError, LEN_MASK};
use core::array::TryFromSliceError;

pub trait PduRead: Sized {
    const LEN: u16;

    type Error: core::fmt::Debug;

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
            fn as_slice<'a>(&'a self) -> &'a [u8] {
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

impl<const N: usize> PduRead for [u8; N] {
    const LEN: u16 = N as u16;

    type Error = TryFromSliceError;

    fn try_from_slice(slice: &[u8]) -> Result<Self, Self::Error> {
        slice.try_into()
    }
}

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
