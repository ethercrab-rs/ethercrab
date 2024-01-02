//! Traits used to pack/unpack structs and enums from EtherCAT packes on the wire.
//!
//! Internal only, please do not implement outside EtherCrab.

// TODO: Can we get rid of PduData and PduRead with these traits?

use crate::error::Error;

macro_rules! impl_primitive_wire_field {
    ($ty:ty, $size:expr) => {
        impl EtherCatWire for $ty {
            const BYTES: usize = $size;

            fn pack_to_slice_unchecked<'buf>(&self, buf: &'buf mut [u8]) -> &'buf [u8] {
                let chunk = &mut buf[0..Self::BYTES];

                chunk.copy_from_slice(&self.to_le_bytes());

                chunk
            }

            fn unpack_from_slice(buf: &[u8]) -> Result<Self, Error> {
                buf.get(0..Self::BYTES)
                    .ok_or(Error::Internal)
                    .and_then(|raw| raw.try_into().map_err(|_| Error::Internal))
                    .map(Self::from_le_bytes)
            }
        }
    };
}

impl_primitive_wire_field!(u8, 1);
impl_primitive_wire_field!(u16, 2);
impl_primitive_wire_field!(u32, 4);

impl EtherCatWire for bool {
    const BYTES: usize = 1;

    fn pack_to_slice_unchecked<'buf>(&self, buf: &'buf mut [u8]) -> &'buf [u8] {
        buf[0] = *self as u8;

        &buf[0..1]
    }

    fn unpack_from_slice(buf: &[u8]) -> Result<Self, Error> {
        if buf.is_empty() {
            return Err(Error::Internal);
        }

        Ok(buf[0] == 1)
    }
}

pub trait EtherCatWire: Sized {
    // const BITS: usize;
    const BYTES: usize;

    fn pack_to_slice<'buf>(&self, buf: &'buf mut [u8]) -> Result<&'buf [u8], Error> {
        if buf.len() < Self::BYTES {
            return Err(Error::Internal);
        }

        Ok(self.pack_to_slice_unchecked(buf))
    }

    fn pack_to_slice_unchecked<'buf>(&self, buf: &'buf mut [u8]) -> &'buf [u8];

    fn unpack_from_slice(buf: &[u8]) -> Result<Self, Error>;
}
