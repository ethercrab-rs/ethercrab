//! Traits used to pack/unpack structs and enums from EtherCAT packes on the wire.
//!
//! Internal only, please do not implement outside EtherCrab.

// pub trait WireEnum {
//     const BITS: usize;
//     const BYTES: usize;
// }

use crate::error::Error;

// pub trait WireField: Into<Self::WireType> + Copy {
//     type WireType;
// }

// impl WireField for u8 {
//     type WireType = Self;
// }
// impl WireField for u16 {
//     type WireType = Self;
// }
// impl WireField for u32 {
//     type WireType = Self;
// }

macro_rules! impl_primitive_wire_field {
    ($ty:ty, $size:expr) => {
        impl WireField for $ty {
            const BYTES: usize = $size;

            fn pack_to_slice_unchecked<'buf>(&self, buf: &'buf mut [u8]) -> &'buf [u8] {
                let chunk = &mut buf[0..Self::BYTES];

                chunk.copy_from_slice(&self.to_le_bytes());

                chunk
            }
        }
    };
}

impl_primitive_wire_field!(u8, 1);
impl_primitive_wire_field!(u16, 2);
impl_primitive_wire_field!(u32, 4);

impl WireField for bool {
    const BYTES: usize = 1;

    fn pack_to_slice_unchecked<'buf>(&self, buf: &'buf mut [u8]) -> &'buf [u8] {
        buf[0] = *self as u8;

        &buf[0..1]
    }
}

pub trait WireField {
    // const BITS: usize;
    const BYTES: usize;

    fn pack_to_slice<'buf>(&self, buf: &'buf mut [u8]) -> Result<&'buf [u8], Error> {
        if buf.len() < Self::BYTES {
            return Err(Error::Internal);
        }

        Ok(self.pack_to_slice_unchecked(buf))
    }

    fn pack_to_slice_unchecked<'buf>(&self, buf: &'buf mut [u8]) -> &'buf [u8];
}
