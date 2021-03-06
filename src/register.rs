use packed_struct::prelude::*;

use crate::PduData;

#[repr(u16)]
pub enum RegisterAddress {
    Type = 0x0000u16,
    Revision = 0x0001,
    Build = 0x0002,
    /// Number of supported FMMU entities.
    FmmuCount = 0x0004,
    /// Number of supported sync manager channels.
    SyncManagerChannels = 0x0005,
    /// RAM size in kilo-octets (1024 octets)
    RamSize = 0x0006,
    // u8
    PortDescriptors = 0x0007,
    // u16
    SupportFlags = 0x0008,
    // u16
    ConfiguredStationAddress = 0x0010,
    // u16
    ConfiguredStationAlias = 0x0012,

    // u8
    // AKA DLS-user R1
    AlControl = 0x0120,
    // u8
    // AKA DLS-user R3
    AlStatus = 0x0130,
    // u16
    // AKA DLS-user R6
    AlStatusCode = 0x0134,
}

impl From<RegisterAddress> for u16 {
    fn from(reg: RegisterAddress) -> Self {
        reg as u16
    }
}

#[derive(Debug, PackedStruct)]
#[packed_struct(bit_numbering = "msb0")]
pub struct PortDescriptors {
    #[packed_field(bits = "0..=1", ty = "enum")]
    port_0: PortType,
    #[packed_field(bits = "2..=3", ty = "enum")]
    port_1: PortType,
    #[packed_field(bits = "4..=5", ty = "enum")]
    port_2: PortType,
    #[packed_field(bits = "6..=7", ty = "enum")]
    port_3: PortType,
}

impl PduData for PortDescriptors {
    const LEN: u16 = 1;

    type Error = packed_struct::PackingError;

    fn try_from_slice(slice: &[u8]) -> Result<Self, Self::Error> {
        let arr = slice[0..1]
            .try_into()
            .map_err(|_| PackingError::BufferTooSmall)?;

        Self::unpack(arr)
    }

    fn as_slice(&self) -> &[u8] {
        todo!()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, PrimitiveEnum_u8)]
#[repr(u8)]
pub enum PortType {
    NotImplemented = 0x00u8,
    NotConfigured = 0x01,
    Ebus = 0x02,
    Mii = 0x03,
}

#[derive(PackedStruct)]
#[packed_struct(bit_numbering = "msb0")]
pub struct SupportFlags {
    #[packed_field(bits = "0")]
    fmmu_supports_bit_ops: bool,
    reserved_register_support: bool,
    dc_supported: bool,
    has_64bit_dc: bool,
    low_jitter: bool,
    ebus_enhanced_link_detection: bool,
    mii_enhanced_link_detection: bool,
    separate_fcs_error_handling: bool,
    enhanced_dc_sync: bool,
    lrw_supported: bool,
    brw_aprw_fprw_supported: bool,
    special_fmmu: bool,
}
