//! Functions used to populate buffers with multiple values.
//!
//! Like cookie_factory but much simpler and will quite happily panic.

/// Write a packed struct into the slice.
pub fn write_packed<T>(value: T, buf: &mut [u8]) -> &mut [u8]
where
    T: ethercrab_wire::EtherCrabWireWrite,
{
    let (buf, rest) = buf.split_at_mut(value.packed_len());

    value.pack_to_slice_unchecked(buf);

    rest
}

/// Skip `n` bytes.
pub fn skip(len: usize, buf: &mut [u8]) -> &mut [u8] {
    let len = len.min(buf.len());

    &mut buf[len..]
}
