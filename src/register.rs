use crate::pdu_data::{PduData, PduRead};
use packed_struct::prelude::*;

#[derive(Copy, Clone, Debug)]
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

    // u16
    SiiConfig = 0x0500,

    // u16
    SiiControl = 0x0502,

    // u16
    SiiAddress = 0x0504,

    // u32 when reading, u16 when writing
    SiiData = 0x0508,

    Fmmu0 = 0x0600,
    Fmmu1 = RegisterAddress::Fmmu0 as u16 + 0x10,
    Fmmu2 = RegisterAddress::Fmmu1 as u16 + 0x10,
    Fmmu3 = RegisterAddress::Fmmu2 as u16 + 0x10,

    Sm0 = 0x0800,
    Sm1 = RegisterAddress::Sm0 as u16 + 0x8,
    Sm2 = RegisterAddress::Sm1 as u16 + 0x8,
    Sm3 = RegisterAddress::Sm2 as u16 + 0x8,

    DcTimePort0 = 0x0900,
}

impl From<RegisterAddress> for u16 {
    fn from(reg: RegisterAddress) -> Self {
        reg as u16
    }
}

impl RegisterAddress {
    pub fn fmmu(index: u8) -> Self {
        match index {
            0 => Self::Fmmu0,
            1 => Self::Fmmu1,
            2 => Self::Fmmu2,
            3 => Self::Fmmu3,
            _ => unreachable!(),
        }
    }

    pub fn sync_manager(index: u8) -> Self {
        match index {
            0 => Self::Sm0,
            1 => Self::Sm1,
            2 => Self::Sm2,
            3 => Self::Sm3,
            _ => unreachable!(),
        }
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

impl PduRead for PortDescriptors {
    const LEN: u16 = 1;

    type Error = packed_struct::PackingError;

    fn try_from_slice(slice: &[u8]) -> Result<Self, Self::Error> {
        let arr = slice[0..1]
            .try_into()
            .map_err(|_| PackingError::BufferTooSmall)?;

        Self::unpack(arr)
    }
}

impl PduData for PortDescriptors {
    fn as_slice(&self) -> &[u8] {
        todo!()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PrimitiveEnum_u8)]
#[repr(u8)]
pub enum PortType {
    NotImplemented = 0x00u8,
    NotConfigured = 0x01,
    Ebus = 0x02,
    Mii = 0x03,
}

#[derive(Debug)]
pub struct SupportFlags {
    // TODO: Un-pub all
    pub fmmu_supports_bit_ops: bool,
    pub reserved_register_support: bool,
    pub dc_supported: bool,
    pub has_64bit_dc: bool,
    pub low_jitter: bool,
    pub ebus_enhanced_link_detection: bool,
    pub mii_enhanced_link_detection: bool,
    pub separate_fcs_error_handling: bool,
    pub enhanced_dc_sync: bool,
    pub lrw_supported: bool,
    pub brw_aprw_fprw_supported: bool,
    pub special_fmmu: bool,
}

impl PackedStruct for SupportFlags {
    type ByteArray = [u8; 2];

    fn pack(&self) -> packed_struct::PackingResult<Self::ByteArray> {
        let result = (self.fmmu_supports_bit_ops as u16) << 0
            & (self.reserved_register_support as u16) << 1
            & (self.dc_supported as u16) << 2
            & (self.has_64bit_dc as u16) << 3
            & (self.low_jitter as u16) << 4
            & (self.ebus_enhanced_link_detection as u16) << 5
            & (self.mii_enhanced_link_detection as u16) << 6
            & (self.separate_fcs_error_handling as u16) << 7
            & (self.enhanced_dc_sync as u16) << 8
            & (self.lrw_supported as u16) << 9
            & (self.brw_aprw_fprw_supported as u16) << 10
            & (self.special_fmmu as u16) << 11;

        Ok(result.to_le_bytes())
    }

    fn unpack(src: &Self::ByteArray) -> packed_struct::PackingResult<Self> {
        let raw = u16::from_le_bytes(*src);

        Ok(Self {
            fmmu_supports_bit_ops: (raw >> 0 & 1) == 1,
            reserved_register_support: (raw >> 1 & 1) == 1,
            dc_supported: (raw >> 2 & 1) == 1,
            has_64bit_dc: (raw >> 3 & 1) == 1,
            low_jitter: (raw >> 4 & 1) == 1,
            ebus_enhanced_link_detection: (raw >> 5 & 1) == 1,
            mii_enhanced_link_detection: (raw >> 6 & 1) == 1,
            separate_fcs_error_handling: (raw >> 7 & 1) == 1,
            enhanced_dc_sync: (raw >> 8 & 1) == 1,
            lrw_supported: (raw >> 9 & 1) == 1,
            brw_aprw_fprw_supported: (raw >> 10 & 1) == 1,
            special_fmmu: (raw >> 11 & 1) == 1,
        })
    }
}

impl PduRead for SupportFlags {
    const LEN: u16 = 2;

    type Error = PackingError;

    fn try_from_slice(slice: &[u8]) -> Result<Self, Self::Error> {
        Self::unpack_from_slice(slice)
    }
}
