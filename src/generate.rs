//! Functions used to populate buffers with multiple values.
//!
//! Like cookie_factory but much simpler and will quite happily panic.

use crate::fmt;
use packed_struct::{PackedStructInfo, PackedStructSlice};

/// Write a `u16`, little-endian.
pub fn le_u16(value: u16, buf: &mut [u8]) -> &mut [u8] {
    let (buf, rest) = buf.split_at_mut(2);

    buf.copy_from_slice(&value.to_le_bytes());

    rest
}

/// Write a `u8`.
pub fn le_u8(value: u8, buf: &mut [u8]) -> &mut [u8] {
    let (buf, rest) = buf.split_at_mut(1);

    buf[0] = value;

    rest
}

/// Write a packed struct into the slice.
pub fn write_packed<T>(value: T, buf: &mut [u8]) -> &mut [u8]
where
    T: PackedStructSlice + PackedStructInfo,
{
    let (buf, rest) = buf.split_at_mut(T::packed_bits() / 8);

    fmt::unwrap!(value
        .pack_to_slice(buf)
        .map_err(crate::error::WrappedPackingError::from));

    rest
}

/// Write a slice into the buffer.
pub fn write_slice<'buf>(value: &[u8], buf: &'buf mut [u8]) -> &'buf mut [u8] {
    let (buf, rest) = buf.split_at_mut(value.len());

    buf.copy_from_slice(value);

    rest
}

/// Skip `n` bytes.
pub fn skip(len: usize, buf: &mut [u8]) -> &mut [u8] {
    let (_, rest) = buf.split_at_mut(len);

    rest
}
