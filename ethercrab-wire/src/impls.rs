//! Builtin implementations for various types.

use crate::{
    EtherCrabWireRead, EtherCrabWireReadSized, EtherCrabWireSized, EtherCrabWireWrite,
    EtherCrabWireWriteSized, WireError,
};

macro_rules! impl_primitive_wire_field {
    ($ty:ty, $size:expr) => {
        impl EtherCrabWireWrite for $ty {
            fn pack_to_slice_unchecked<'buf>(&self, buf: &'buf mut [u8]) -> &'buf [u8] {
                // This unsafe doesn't save us any binary space at all in the stm32-embassy example
                // so we won't use it.
                // let chunk = unsafe { buf.get_unchecked_mut(0..$size) };

                let chunk = &mut buf[0..$size];

                chunk.copy_from_slice(&self.to_le_bytes());

                chunk
            }

            fn pack_to_slice<'buf>(&self, buf: &'buf mut [u8]) -> Result<&'buf [u8], WireError> {
                if buf.len() < $size {
                    return Err(WireError::WriteBufferTooShort {
                        expected: $size,
                        got: buf.len(),
                    });
                }

                Ok(self.pack_to_slice_unchecked(buf))
            }

            fn packed_len(&self) -> usize {
                $size
            }
        }

        impl EtherCrabWireRead for $ty {
            fn unpack_from_slice(buf: &[u8]) -> Result<Self, WireError> {
                buf.get(0..$size)
                    .ok_or(WireError::ReadBufferTooShort {
                        expected: $size,
                        got: buf.len(),
                    })
                    .map(|raw| match raw.try_into() {
                        Ok(res) => res,
                        Err(_) => unreachable!(),
                    })
                    .map(Self::from_le_bytes)
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

impl_primitive_wire_field!(u8, 1);
impl_primitive_wire_field!(u16, 2);
impl_primitive_wire_field!(u32, 4);
impl_primitive_wire_field!(u64, 8);
impl_primitive_wire_field!(i8, 1);
impl_primitive_wire_field!(i16, 2);
impl_primitive_wire_field!(i32, 4);
impl_primitive_wire_field!(i64, 8);

impl EtherCrabWireWrite for bool {
    fn pack_to_slice_unchecked<'buf>(&self, buf: &'buf mut [u8]) -> &'buf [u8] {
        buf[0] = *self as u8;

        &buf[0..1]
    }

    fn packed_len(&self) -> usize {
        1
    }
}

impl EtherCrabWireRead for bool {
    fn unpack_from_slice(buf: &[u8]) -> Result<Self, WireError> {
        if buf.is_empty() {
            return Err(WireError::ReadBufferTooShort {
                expected: 1,
                got: 0,
            });
        }

        Ok(buf[0] == 1)
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
        [*self as u8; 1]
    }
}

impl EtherCrabWireWrite for () {
    fn pack_to_slice_unchecked<'buf>(&self, buf: &'buf mut [u8]) -> &'buf [u8] {
        &buf[0..0]
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
        let buf = &mut buf[0..N];

        buf.copy_from_slice(self);

        buf
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
        if buf.len() < T::PACKED_LEN * N {
            return Err(WireError::ReadBufferTooShort {
                expected: T::PACKED_LEN * N,
                got: buf.len(),
            });
        }

        heapless::Vec::<T, N>::unpack_from_slice(buf)
            .and_then(|res| res.into_array().map_err(|_e| WireError::ArrayLength))
    }
}

// --- heapless::Vec ---

impl<const N: usize, T> EtherCrabWireRead for heapless::Vec<T, N>
where
    T: EtherCrabWireReadSized,
{
    fn unpack_from_slice(buf: &[u8]) -> Result<Self, WireError> {
        let res = buf
            .chunks_exact(T::PACKED_LEN)
            .take(N)
            .map(T::unpack_from_slice)
            .collect::<Result<heapless::Vec<_, N>, WireError>>();

        match res {
            Ok(res) => Ok(res),
            Err(_) => unreachable!(),
        }
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
