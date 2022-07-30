//! Slave Information Interface (SII).

use packed_struct::prelude::*;

use crate::PduRead;

/// Defined in ETG1000.4 6.4.3
#[derive(Debug, Copy, Clone, PartialEq, Default, PackedStruct)]
#[packed_struct(size_bytes = "2", bit_numbering = "lsb0", endian = "lsb")]
pub struct SiiControl {
    // First byte, but second octet because little endian
    #[packed_field(bits = "8", ty = "enum")]
    pub access: SiiAccess,
    // #[packed_field(bits = "9..=12")]
    // reserved4: u8,
    #[packed_field(bits = "13")]
    pub emulate_sii: bool,
    #[packed_field(bits = "14", ty = "enum")]
    pub read_size: SiiReadSize,
    #[packed_field(bits = "15", ty = "enum")]
    pub address_type: SiiAddressSize,

    // Second byte, but first octet because little endian
    // TODO: Replace with bitflags struct?
    #[packed_field(bits = "0")]
    pub read: bool,
    #[packed_field(bits = "1")]
    pub write: bool,
    #[packed_field(bits = "2")]
    pub reload: bool,
    #[packed_field(bits = "3")]
    pub checksum_error: bool,
    #[packed_field(bits = "4")]
    pub device_info_error: bool,
    #[packed_field(bits = "5")]
    pub command_error: bool,
    #[packed_field(bits = "6")]
    pub write_error: bool,
    #[packed_field(bits = "7")]
    pub busy: bool,
}

impl SiiControl {
    fn read() -> Self {
        Self {
            read: true,
            ..Default::default()
        }
    }
}

impl PduRead for SiiControl {
    const LEN: u16 = u16::LEN;

    type Error = PackingError;

    fn try_from_slice(slice: &[u8]) -> Result<Self, Self::Error> {
        Self::unpack_from_slice(slice)
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Default, PrimitiveEnum_u8)]
pub enum SiiAccess {
    #[default]
    ReadOnly = 0x00,
    ReadWrite = 0x01,
}

#[derive(Debug, Copy, Clone, PartialEq, Default, PrimitiveEnum_u8)]
pub enum SiiReadSize {
    #[default]
    Bits4 = 0x00,
    Bits8 = 0x01,
}

#[derive(Debug, Copy, Clone, PartialEq, Default, PrimitiveEnum_u8)]
pub enum SiiAddressSize {
    #[default]
    U8 = 0x00,
    U16 = 0x01,
}

pub struct SiiRequest {
    control: SiiControl,
    address: u16,
}

impl SiiRequest {
    pub fn read(address: u16) -> Self {
        Self {
            control: SiiControl::read(),
            address,
        }
    }

    pub fn to_array(self) -> [u8; 4] {
        let mut buf = [0u8; 4];

        self.control.pack_to_slice(&mut buf[0..2]).unwrap();

        buf[2..4].copy_from_slice(&self.address.to_le_bytes());

        buf
    }
}
