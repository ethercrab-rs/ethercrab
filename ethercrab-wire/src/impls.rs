//! Builtin implementations for various types.

use crate::{EtherCrabWireRead, EtherCrabWireReadWrite, EtherCrabWireReadWriteSized, WireError};

macro_rules! impl_primitive_wire_field {
    ($ty:ty, $size:expr) => {
        impl EtherCrabWireReadWrite for $ty {
            fn pack_to_slice_unchecked<'buf>(&self, buf: &'buf mut [u8]) -> &'buf [u8] {
                let chunk = &mut buf[0..$size];

                chunk.copy_from_slice(&self.to_le_bytes());

                chunk
            }

            fn pack_to_slice<'buf>(&self, buf: &'buf mut [u8]) -> Result<&'buf [u8], WireError> {
                if buf.len() < $size {
                    return Err(WireError::Todo);
                }

                Ok(self.pack_to_slice_unchecked(buf))
            }
        }

        impl EtherCrabWireRead for $ty {
            fn unpack_from_slice(buf: &[u8]) -> Result<Self, WireError> {
                buf.get(0..$size)
                    .ok_or(WireError::Todo)
                    .and_then(|raw| raw.try_into().map_err(|_| WireError::Todo))
                    .map(Self::from_le_bytes)
            }

            fn packed_len(&self) -> usize {
                $size
            }
        }

        impl EtherCrabWireReadWriteSized for $ty {
            const PACKED_LEN: usize = $size;

            type Buffer = [u8; $size];

            fn pack(&self) -> Self::Buffer {
                self.to_le_bytes()
            }

            fn buffer() -> Self::Buffer {
                [0u8; $size]
            }
        }
    };
}

impl_primitive_wire_field!(u8, 1);
impl_primitive_wire_field!(u16, 2);
impl_primitive_wire_field!(u32, 4);
impl_primitive_wire_field!(u64, 8);
impl_primitive_wire_field!(i8, 1);
impl_primitive_wire_field!(i16, 2);
impl_primitive_wire_field!(i32, 4);
impl_primitive_wire_field!(i64, 8);

impl EtherCrabWireReadWrite for bool {
    fn pack_to_slice_unchecked<'buf>(&self, buf: &'buf mut [u8]) -> &'buf [u8] {
        buf[0] = *self as u8;

        &buf[0..1]
    }
}

impl EtherCrabWireRead for bool {
    fn unpack_from_slice(buf: &[u8]) -> Result<Self, WireError> {
        if buf.is_empty() {
            return Err(WireError::Todo);
        }

        Ok(buf[0] == 1)
    }
    fn packed_len(&self) -> usize {
        1
    }
}

impl EtherCrabWireReadWriteSized for bool {
    const PACKED_LEN: usize = 1;

    type Buffer = [u8; Self::PACKED_LEN];

    fn pack(&self) -> Self::Buffer {
        [*self as u8; 1]
    }

    fn buffer() -> Self::Buffer {
        [0u8; 1]
    }
}

impl EtherCrabWireReadWrite for () {
    fn pack_to_slice_unchecked<'buf>(&self, buf: &'buf mut [u8]) -> &'buf [u8] {
        &buf[0..0]
    }
}

impl EtherCrabWireRead for () {
    fn unpack_from_slice(_buf: &[u8]) -> Result<Self, WireError> {
        Ok(())
    }

    fn packed_len(&self) -> usize {
        0
    }
}

impl EtherCrabWireReadWriteSized for () {
    const PACKED_LEN: usize = 0;

    type Buffer = [u8; 0];

    fn pack(&self) -> Self::Buffer {
        [0u8; 0]
    }

    fn buffer() -> Self::Buffer {
        [0u8; 0]
    }
}

impl<const N: usize> EtherCrabWireReadWrite for [u8; N] {
    fn pack_to_slice_unchecked<'buf>(&self, buf: &'buf mut [u8]) -> &'buf [u8] {
        let buf = &mut buf[0..N];

        buf.copy_from_slice(self);

        buf
    }
}

impl<const N: usize> EtherCrabWireRead for [u8; N] {
    fn unpack_from_slice(buf: &[u8]) -> Result<Self, WireError> {
        let chunk = buf.get(0..N).ok_or(WireError::Todo)?;

        chunk.try_into().map_err(|_e| WireError::Todo)
    }

    fn packed_len(&self) -> usize {
        N
    }
}

impl<const N: usize> EtherCrabWireReadWriteSized for [u8; N] {
    const PACKED_LEN: usize = N;

    type Buffer = [u8; N];

    fn pack(&self) -> Self::Buffer {
        *self
    }

    fn buffer() -> Self::Buffer {
        [0u8; N]
    }
}
