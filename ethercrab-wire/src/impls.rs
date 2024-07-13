//! Builtin implementations for various types.

use crate::{
    EtherCrabWireRead, EtherCrabWireReadSized, EtherCrabWireSized, EtherCrabWireWrite,
    EtherCrabWireWriteSized, WireError,
};

macro_rules! impl_primitive_wire_field {
    ($ty:ty, $size:expr) => {
        impl EtherCrabWireWrite for $ty {
            fn pack_to_slice_unchecked<'buf>(&self, buf: &'buf mut [u8]) -> &'buf [u8] {
                let Some(chunk) = buf.first_chunk_mut::<$size>() else {
                    unreachable!()
                };

                *chunk = self.to_le_bytes();

                chunk
            }

            fn pack_to_slice<'buf>(&self, buf: &'buf mut [u8]) -> Result<&'buf [u8], WireError> {
                let Some(chunk) = buf.first_chunk_mut::<$size>() else {
                    return Err(WireError::WriteBufferTooShort);
                };

                *chunk = self.to_le_bytes();

                Ok(chunk)
            }

            fn packed_len(&self) -> usize {
                $size
            }
        }

        impl EtherCrabWireRead for $ty {
            fn unpack_from_slice(buf: &[u8]) -> Result<Self, WireError> {
                buf.first_chunk::<$size>()
                    .ok_or(WireError::ReadBufferTooShort)
                    .map(|chunk| Self::from_le_bytes(*chunk))
            }
        }

        impl EtherCrabWireSized for $ty {
            const PACKED_LEN: usize = $size;

            type Buffer = [u8; $size];

            fn buffer() -> Self::Buffer {
                [0u8; $size]
            }
        }

        impl EtherCrabWireWriteSized for $ty {
            fn pack(&self) -> Self::Buffer {
                self.to_le_bytes()
            }
        }

        // MSRV: generic_const_exprs: Once we can do `N * T::PACKED_BYTES` this impl can go away and
        // be replaced by a single generic one.
        impl<const N: usize> EtherCrabWireSized for [$ty; N] {
            const PACKED_LEN: usize = N * $size;

            type Buffer = [u8; N];

            fn buffer() -> Self::Buffer {
                [0u8; N]
            }
        }
    };
}

// Thank you `serde::Deserialize` :D
macro_rules! impl_tuples {
    ($($len:tt => ($($n:tt $name:ident)+))+) => {
        $(
            #[allow(non_snake_case)]
            impl<$($name: EtherCrabWireReadSized),+> EtherCrabWireRead for ($($name,)+) {
                #[allow(unused_assignments)]
                fn unpack_from_slice(mut buf: &[u8]) -> Result<Self, WireError> {
                    $(
                        let $name = $name::unpack_from_slice(buf)?;

                        if buf.len() > 0 {
                            buf = &buf[$name::PACKED_LEN..];
                        }
                    )+

                    Ok(($($name,)+))
                }
            }

            #[allow(non_snake_case)]
            impl<$($name: EtherCrabWireWrite),+> EtherCrabWireWrite for ($($name,)+) {
                #[allow(unused_assignments)]
                fn pack_to_slice_unchecked<'buf>(&self, orig: &'buf mut [u8]) -> &'buf [u8] {
                    {
                        let mut buf = &mut orig[..];

                        $(
                            let (chunk, rest) = buf.split_at_mut(self.$n.packed_len());
                            let _packed = self.$n.pack_to_slice_unchecked(chunk);
                            buf = rest;
                        )+
                    }

                    &orig[0..self.packed_len()]
                }

                fn packed_len(&self) -> usize {
                    0
                    $(
                        + self.$n.packed_len()
                    )+
                }
            }
        )+
    }
}

impl_tuples! {
    1  => (0 T0)
    2  => (0 T0 1 T1)
    3  => (0 T0 1 T1 2 T2)
    4  => (0 T0 1 T1 2 T2 3 T3)
    5  => (0 T0 1 T1 2 T2 3 T3 4 T4)
    6  => (0 T0 1 T1 2 T2 3 T3 4 T4 5 T5)
    7  => (0 T0 1 T1 2 T2 3 T3 4 T4 5 T5 6 T6)
    8  => (0 T0 1 T1 2 T2 3 T3 4 T4 5 T5 6 T6 7 T7)
    9  => (0 T0 1 T1 2 T2 3 T3 4 T4 5 T5 6 T6 7 T7 8 T8)
    10 => (0 T0 1 T1 2 T2 3 T3 4 T4 5 T5 6 T6 7 T7 8 T8 9 T9)
    11 => (0 T0 1 T1 2 T2 3 T3 4 T4 5 T5 6 T6 7 T7 8 T8 9 T9 10 T10)
    12 => (0 T0 1 T1 2 T2 3 T3 4 T4 5 T5 6 T6 7 T7 8 T8 9 T9 10 T10 11 T11)
    13 => (0 T0 1 T1 2 T2 3 T3 4 T4 5 T5 6 T6 7 T7 8 T8 9 T9 10 T10 11 T11 12 T12)
    14 => (0 T0 1 T1 2 T2 3 T3 4 T4 5 T5 6 T6 7 T7 8 T8 9 T9 10 T10 11 T11 12 T12 13 T13)
    15 => (0 T0 1 T1 2 T2 3 T3 4 T4 5 T5 6 T6 7 T7 8 T8 9 T9 10 T10 11 T11 12 T12 13 T13 14 T14)
    16 => (0 T0 1 T1 2 T2 3 T3 4 T4 5 T5 6 T6 7 T7 8 T8 9 T9 10 T10 11 T11 12 T12 13 T13 14 T14 15 T15)
}

impl_primitive_wire_field!(u8, 1);
impl_primitive_wire_field!(u16, 2);
impl_primitive_wire_field!(u32, 4);
impl_primitive_wire_field!(u64, 8);
impl_primitive_wire_field!(i8, 1);
impl_primitive_wire_field!(i16, 2);
impl_primitive_wire_field!(i32, 4);
impl_primitive_wire_field!(i64, 8);

impl_primitive_wire_field!(f32, 4);
impl_primitive_wire_field!(f64, 8);

impl EtherCrabWireWrite for bool {
    fn pack_to_slice_unchecked<'buf>(&self, buf: &'buf mut [u8]) -> &'buf [u8] {
        buf[0] = if *self { 0xff } else { 0x00 };

        &buf[0..1]
    }

    fn packed_len(&self) -> usize {
        1
    }
}

impl EtherCrabWireRead for bool {
    fn unpack_from_slice(buf: &[u8]) -> Result<Self, WireError> {
        // NOTE: ETG1000.6 5.2.2 states the truthy value is 0xff and false is 0. We'll just check
        // for greater than zero to be sure.
        Ok(*buf.first().ok_or(WireError::ReadBufferTooShort)? > 0)
    }
}

impl EtherCrabWireSized for bool {
    const PACKED_LEN: usize = 1;

    type Buffer = [u8; Self::PACKED_LEN];

    fn buffer() -> Self::Buffer {
        [0u8; 1]
    }
}

impl EtherCrabWireWriteSized for bool {
    fn pack(&self) -> Self::Buffer {
        // NOTE: ETG1000.6 5.2.2 states the truthy value is 0xff and false is 0.
        [if *self { 0xff } else { 0x00 }; 1]
    }
}

impl EtherCrabWireWrite for () {
    fn pack_to_slice_unchecked<'buf>(&self, _buf: &'buf mut [u8]) -> &'buf [u8] {
        &[]
    }

    fn packed_len(&self) -> usize {
        0
    }
}

impl EtherCrabWireRead for () {
    fn unpack_from_slice(_buf: &[u8]) -> Result<Self, WireError> {
        Ok(())
    }
}

impl EtherCrabWireSized for () {
    const PACKED_LEN: usize = 0;

    type Buffer = [u8; 0];

    fn buffer() -> Self::Buffer {
        [0u8; 0]
    }
}

impl EtherCrabWireWriteSized for () {
    fn pack(&self) -> Self::Buffer {
        [0u8; 0]
    }
}

impl<const N: usize> EtherCrabWireWrite for [u8; N] {
    fn pack_to_slice_unchecked<'buf>(&self, buf: &'buf mut [u8]) -> &'buf [u8] {
        let Some(chunk) = buf.first_chunk_mut::<N>() else {
            unreachable!()
        };

        *chunk = *self;

        chunk
    }

    fn packed_len(&self) -> usize {
        N
    }
}

impl EtherCrabWireWrite for &[u8] {
    fn pack_to_slice_unchecked<'buf>(&self, buf: &'buf mut [u8]) -> &'buf [u8] {
        let buf = &mut buf[0..self.len()];

        buf.copy_from_slice(self);

        buf
    }

    fn packed_len(&self) -> usize {
        self.len()
    }
}

// Blanket impl for references
impl<T> EtherCrabWireWrite for &T
where
    T: EtherCrabWireWrite,
{
    fn pack_to_slice_unchecked<'buf>(&self, buf: &'buf mut [u8]) -> &'buf [u8] {
        EtherCrabWireWrite::pack_to_slice_unchecked(*self, buf)
    }

    fn packed_len(&self) -> usize {
        EtherCrabWireWrite::packed_len(*self)
    }
}

// Blanket impl for arrays of known-sized types
impl<const N: usize, T> EtherCrabWireRead for [T; N]
where
    T: EtherCrabWireReadSized,
{
    fn unpack_from_slice(buf: &[u8]) -> Result<Self, WireError> {
        buf.get(0..(T::PACKED_LEN * N))
            .ok_or(WireError::ReadBufferTooShort)?
            .chunks_exact(T::PACKED_LEN)
            .take(N)
            .map(T::unpack_from_slice)
            .collect::<Result<heapless::Vec<_, N>, WireError>>()
            .and_then(|res| res.into_array().map_err(|_e| WireError::ArrayLength))
    }
}

// --- heapless::Vec ---

impl<const N: usize, T> EtherCrabWireRead for heapless::Vec<T, N>
where
    T: EtherCrabWireReadSized,
{
    fn unpack_from_slice(buf: &[u8]) -> Result<Self, WireError> {
        buf.chunks_exact(T::PACKED_LEN)
            .take(N)
            .map(T::unpack_from_slice)
            .collect::<Result<heapless::Vec<_, N>, WireError>>()
    }
}

// MSRV: generic_const_exprs: When we can do `N * T::PACKED_LEN`, this specific impl for `u8` can be
// replaced with `T: EtherCrabWireSized`.
impl<const N: usize, T> EtherCrabWireSized for heapless::Vec<T, N>
where
    T: Into<u8>,
{
    const PACKED_LEN: usize = N;

    type Buffer = [u8; N];

    fn buffer() -> Self::Buffer {
        [0u8; N]
    }
}

// --- heapless::String ---

impl<const N: usize> EtherCrabWireRead for heapless::String<N> {
    fn unpack_from_slice(buf: &[u8]) -> Result<Self, WireError> {
        core::str::from_utf8(buf)
            .map_err(|_| WireError::InvalidUtf8)
            .and_then(|s| Self::try_from(s).map_err(|_| WireError::ArrayLength))
    }
}

impl<const N: usize> EtherCrabWireSized for heapless::String<N> {
    const PACKED_LEN: usize = N;

    type Buffer = [u8; N];

    fn buffer() -> Self::Buffer {
        [0u8; N]
    }
}

// --- std ---

#[cfg(feature = "std")]
impl EtherCrabWireRead for String {
    fn unpack_from_slice(buf: &[u8]) -> Result<Self, WireError> {
        core::str::from_utf8(buf)
            .map_err(|_| WireError::InvalidUtf8)
            .map(String::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bool_pack() {
        assert_eq!(true.pack(), [0xff]);
        assert_eq!(false.pack(), [0x00]);

        let mut sl1 = [0u8; 8];
        let mut sl2 = [0u8; 8];

        assert_eq!(true.pack_to_slice_unchecked(&mut sl1), &[0xffu8]);
        assert_eq!(false.pack_to_slice_unchecked(&mut sl2), &[0x00u8]);
    }

    #[test]
    fn bool_unpack() {
        assert_eq!(bool::unpack_from_slice(&[0xff]), Ok(true));
        assert_eq!(bool::unpack_from_slice(&[0x00]), Ok(false));

        // In case there are noncompliant subdevices
        assert_eq!(bool::unpack_from_slice(&[0x01]), Ok(true));
    }

    #[test]
    fn tuple_decode() {
        let res = <(u32, u8)>::unpack_from_slice(&[0xaa, 0xbb, 0xcc, 0xdd, 0x99]);

        assert_eq!(
            res,
            Ok((u32::from_le_bytes([0xaa, 0xbb, 0xcc, 0xdd]), 0x99))
        )
    }

    #[test]
    fn tuple_encode() {
        let mut buf = [0u8; 32];

        let written = (0xaabbccddu32, 0x99u8, 0x1234u16).pack_to_slice_unchecked(&mut buf);

        assert_eq!(written, &[0xdd, 0xcc, 0xbb, 0xaa, 0x99, 0x34, 0x12]);
    }
}
