//! Traits used to pack/unpack structs and enums from EtherCAT packes on the wire.
//!
//! Internal only, please do not implement outside EtherCrab.

// pub trait WireEnum {
//     const BITS: usize;
//     const BYTES: usize;
// }

use crate::error::Error;

pub trait WireField: Into<Self::WireType> + Copy {
    type WireType;
}

impl WireField for u8 {
    type WireType = Self;
}
impl WireField for u16 {
    type WireType = Self;
}
impl WireField for u32 {
    type WireType = Self;
}

pub trait WireStruct {
    const BITS: usize;
    const BYTES: usize;

    fn pack_to_slice<'buf>(&self, buf: &'buf mut [u8]) -> Result<&'buf [u8], Error>;
}
