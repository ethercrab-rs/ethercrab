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
                    return Err(WireError::Todo);
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
                    .ok_or(WireError::Todo)
                    .and_then(|raw| raw.try_into().map_err(|_| WireError::Todo))
                    .map(Self::from_le_bytes)

                // TODO: Write a very quick blog post on the fact that this doesn't improve
                // generated code at all, and how nice Rust is!
                //
                // Godbolt here:
                // https://godbolt.org/#z:OYLghAFBqd5TKALEBjA9gEwKYFFMCWALugE4A0BIEAZgQDbYB2AhgLbYgDkAjF%2BTXRMiAZVQtGIHgBYBQogFUAztgAKAD24AGfgCsp5eiyahSAVyVFyKxqiIEh1ZpgDC6embZMQAJgAc5M4AMgRM2AByngBG2KQgAMzkAA7oSsQOTG4eXr4BKWn2QiFhkWwxcYk22HYZIkQspERZnt7%2B1ti2hUx1DUTFEdGxCdb1jc05bZa9/aXlCQCU1uhmpKicXACkPvFgYBsArABCTOgA%2BpaYBwAiG1oAgrd3FtgA1BiknGhCAG6xRCAgAAqpAAngBJYToDbxQ6PR5JMxRF40JgvMxMJIsVAAa1ONFI6DY53oBDWpySjBYSmMRAgUTMNBALy2ADYDoczH5rvMXgBaaG4F4AJWwSjM9CI0JcZniPnILwg8wFzIA7LD7i9NS96TQAHTAbC0rS63XSJUarWW3XoXFkKDzc13S1W4yYU5EJDMCAbFUuUgsADuPpcL39Ad1RFBp1CJEVurYLCSp1ipG9vtOwYVDsdzq18cTEBlPgB%2BMJp0YpyiIKIosdPquXEW9G4%2B343i4OnI6G4QosRBeSmWq1eW3ifHI/w7jcW2JA%2By0hm40n4bDnC/bne7XH4ShAC8nOkWcFgKAwbCSDFilGoZ4vjDipB4KvnAgYNdIu7p2n4UVCDRB3Djr%2BrCggA8lEujVJO45nhwwigUw9AAVO5A4PSwAuBI9C7rw/A4AmJiSChhAfDUvw4Z22DqNUZg1oB/Axh036GAQUT%2BqCbg4MxkYEKuuGLDQRjAEoABqBDYAGoFJMw9FyMIYgSJwMhyYoKgaMx%2Bg8IYximH2LFRLukCLOgSRdDhvKgXyvKjAa/YetE3wqmwvInGEvLYCwv78OgvykKQBA4IZUCsBwIDYIQXTkN8EhmOsPhaD4fAOu0nQZE4TCuO4LQGMEoQDGUQxafk6RCOM3hFakJVMDMgxxFpVQ1EIPRjFlOT1R0UG1KMfR5bMhUjL0ZUGFMjQ1QVdWLIOKxrFITYtm2zFbqGfaoC8PC6s%2BupaAq%2BDEGQzLbDw8z8Ae07kJ6LA4HEirkLOL7Nlwy7kBu3ncDue4Tt%2ByUPT4K5rs9i1vZ9U5Hogx7IF8OC7SQFBULQl4hes46CPJ4iSMpKOqWomgofocpGCYIDmJYKWdY4EDOENWm5SUtVzsklVdFTDMFBkY1zPspONd03XMw1XTNT1tPjfTI1NK15UDaNvV0/sk1DjNmzbLs7InOcRCXPsNz3I8zxvGQnwYEwvn/ECoIQiQ0Lqk8Kj6x8AJIDGALoh8WJIJ5Fboqgno4uFVtwvcCJIiiaIYliuKlkSSgkmSrAEL8dIMkyrLspy3J8sqIpihKUpFvKirKj61uWgQNDagyuqMEwirMvEIbSKqxe5qGhorKiuB%2BfaSowo8lr1gHTpaow/YNKQtdXC8CZEN75c0OyxqmtcEZRjG6A10XvfN6B2IQKPPLQjc8SCqP5Cb7mAD058vCIdwAGK4ICACaTIAOqvN71TYi8HqvDqNCxAOAgAAvV4nkfLYDPs6DuqZTj73iIfQU6JqT/0bqHV23sPbYFOF7H22JwrrxVFcU%2BFpNT9x7jrEhLxt6FllCWAkRIKxVhrEoXefkHRwkIY2RcXBWwAxQluXslgBwKxHNsX6wNDyLAuldagM5/oPSequF8L0uxA13PuL65BwYQFPISO8V44a3kvA%2BJ8L46ASliJ%2BKIzFgL/lkrYsCEEoKyVgswIgCEkLMTQmYDCWEcLjnwjpIinYSKdXIsxKiNE6K4UoMIJiKESRsX/JxdYnYeJ8SSgIISolxKSWku2ZG8gFLo1kJjZQ2MNK%2BG0oTYmVhElBWMqZDI5lLK8msg0Wy38kAOSci5IQ2B3KeQIN5Xy/lArwAgIjMKEUMhRRinFBKSVJodW5ulTK2RJY03ynMCqrNSoSwMMVLo7N%2Br8y6oNA57VUpNW6icuqUtxYbOGrcmWIsjpLGmpwHwc0eELX4dwZalhVrrU2ttCA0N9qjm%2BSdL6UiPIyJundBcCi/mbjUR9U631uDiKUeuQG24JFnQevEVFr0CWYsWL5NIjhpBAA%3D%3D

                // if buf.len() < $size {
                //     return Err(WireError::Todo);
                // }

                // let arr = match buf[0..$size].try_into() {
                //     Ok(arr) => arr,
                //     // SAFETY: We check the buffer size above
                //     Err(_) => unsafe { unreachable_unchecked() },
                // };

                // Ok(<$ty>::from_le_bytes(arr))
            }

            // fn unpack_from_slice_rest<'buf>(
            //     buf: &'buf [u8],
            // ) -> Result<(Self, &'buf [u8]), WireError> {
            //     if buf.len() < $size {
            //         return Err(WireError::Todo);
            //     }

            //     let (raw, rest) = buf.split_at($size);

            //     raw.try_into()
            //         .map_err(|_| WireError::Todo)
            //         .map(|n| (Self::from_le_bytes(n), rest))
            // }
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
            return Err(WireError::Todo);
        }

        Ok(buf[0] == 1)
    }

    // fn unpack_from_slice_rest<'buf>(buf: &'buf [u8]) -> Result<(Self, &'buf [u8]), WireError> {
    //     if buf.is_empty() {
    //         return Err(WireError::Todo);
    //     }

    //     let (buf, rest) = buf.split_at(1);

    //     Ok((buf[0] == 1, rest))
    // }
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

    // fn unpack_from_slice_rest<'buf>(buf: &'buf [u8]) -> Result<(Self, &'buf [u8]), WireError> {
    //     Ok(((), buf))
    // }
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
            return Err(WireError::Todo);
        }

        heapless::Vec::<T, N>::unpack_from_slice(buf)
            .and_then(|res| res.into_array().map_err(|_e| WireError::Todo))
    }
}

// Heapless crate support
impl<const N: usize, T> EtherCrabWireRead for heapless::Vec<T, N>
where
    T: EtherCrabWireReadSized,
{
    fn unpack_from_slice(buf: &[u8]) -> Result<Self, WireError> {
        buf.chunks_exact(T::PACKED_LEN)
            .map(T::unpack_from_slice)
            .collect::<Result<heapless::Vec<_, N>, WireError>>()
    }
}

// MSRV: generic_const_exprs: When we can do `N * T::PACKED_LEN`, this specific impl bounded on
// TryFrom<u8> can go away.
impl<const N: usize, T> EtherCrabWireSized for [T; N]
where
    T: TryFrom<u8>,
{
    const PACKED_LEN: usize = N;

    type Buffer = [u8; N];

    fn buffer() -> Self::Buffer {
        [0u8; N]
    }
}

// MSRV: generic_const_exprs: When we can do `N * T::PACKED_LEN`, this specific impl bounded on
// TryFrom<u8> can go away.
impl<const N: usize, T> EtherCrabWireSized for heapless::Vec<T, N>
where
    T: TryFrom<u8>,
{
    const PACKED_LEN: usize = N;

    type Buffer = [u8; N];

    fn buffer() -> Self::Buffer {
        [0u8; N]
    }
}
