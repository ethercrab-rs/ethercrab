use crate::{
    error::WrappedPackingError,
    pdu_data::{PduData, PduRead},
};
use packed_struct::{prelude::*, PackingError};

/// Slave device register address abstraction.
///
/// This enum makes it easier to work with raw EtherCAT addresses by giving them nice names.
///
/// Defined in ETG1000.4, Table 31.
#[derive(Copy, Clone, Debug)]
#[repr(u16)]
pub enum RegisterAddress {
    /// Type.
    Type = 0x0000u16,
    /// EtherCAT revision.
    Revision = 0x0001,
    /// Slave build.
    Build = 0x0002,
    /// Number of supported FMMU entities.
    FmmuCount = 0x0004,
    /// Number of supported sync manager channels.
    SyncManagerChannels = 0x0005,
    /// RAM size in kilo-octets (1024 octets)
    RamSize = 0x0006,
    // u8
    /// EtherCAT port descriptors 0-3
    PortDescriptors = 0x0007,
    // u16
    /// Different EtherCAT features supported by the slave.
    SupportFlags = 0x0008,
    // u16
    /// The slave's configured station address.
    ConfiguredStationAddress = 0x0010,
    // u16
    /// The slave's address alias.
    ConfiguredStationAlias = 0x0012,

    // u16
    /// Defined in ETG1000.4 Table 34 - DL status.
    DlStatus = 0x0110,

    // u8
    // AKA DLS-user R1
    /// Application Layer (AL) control register.
    AlControl = 0x0120,
    // u8
    // AKA DLS-user R3
    /// Application Layer (AL) status register.
    AlStatus = 0x0130,
    // u16
    // AKA DLS-user R6
    /// Application Layer (AL) status code register.
    AlStatusCode = 0x0134,

    // u16
    /// EEPROM (SII) config register.
    SiiConfig = 0x0500,

    // u16
    /// EEPROM (SII) control register.
    SiiControl = 0x0502,

    // u16
    /// EEPROM (SII) control address.
    SiiAddress = 0x0504,

    // u32 when reading, u16 when writing
    /// The start of 4 bytes (read) or 2 bytes (write) of data used by the EEPROM read/write
    /// interface.
    SiiData = 0x0508,

    /// Fieldbus Memory Management Unit (FMMU) 0.
    ///
    /// Defined in ETG1000.4 Table 57
    Fmmu0 = 0x0600,
    /// Fieldbus Memory Management Unit (FMMU) 1.
    Fmmu1 = 0x0610,
    /// Fieldbus Memory Management Unit (FMMU) 2.
    Fmmu2 = 0x0620,
    /// Fieldbus Memory Management Unit (FMMU) 3.
    Fmmu3 = 0x0630,
    /// Fieldbus Memory Management Unit (FMMU) 4.
    Fmmu4 = 0x0640,
    /// Fieldbus Memory Management Unit (FMMU) 5.
    Fmmu5 = 0x0650,
    /// Fieldbus Memory Management Unit (FMMU) 6.
    Fmmu6 = 0x0660,
    /// Fieldbus Memory Management Unit (FMMU) 7.
    Fmmu7 = 0x0670,
    /// Fieldbus Memory Management Unit (FMMU) 8.
    Fmmu8 = 0x0680,
    /// Fieldbus Memory Management Unit (FMMU) 9.
    Fmmu9 = 0x0690,
    /// Fieldbus Memory Management Unit (FMMU) 10.
    Fmmu10 = 0x06A0,
    /// Fieldbus Memory Management Unit (FMMU) 11.
    Fmmu11 = 0x06B0,
    /// Fieldbus Memory Management Unit (FMMU) 12.
    Fmmu12 = 0x06C0,
    /// Fieldbus Memory Management Unit (FMMU) 13.
    Fmmu13 = 0x06D0,
    /// Fieldbus Memory Management Unit (FMMU) 14.
    Fmmu14 = 0x06E0,
    /// Fieldbus Memory Management Unit (FMMU) 15.
    Fmmu15 = 0x06F0,

    /// Sync Manager (SM) 0.
    ///
    /// Defined in ETG1000.4 Table 59.
    Sm0 = 0x0800,
    /// Sync Manager (SM) 1.
    Sm1 = 0x0808,
    /// Sync Manager (SM) 2.
    Sm2 = 0x0810,
    /// Sync Manager (SM) 3.
    Sm3 = 0x0818,
    /// Sync Manager (SM) 4.
    Sm4 = 0x0820,
    /// Sync Manager (SM) 5.
    Sm5 = 0x0828,
    /// Sync Manager (SM) 6.
    Sm6 = 0x0830,
    /// Sync Manager (SM) 7.
    Sm7 = 0x0838,
    /// Sync Manager (SM) 8.
    Sm8 = 0x0840,
    /// Sync Manager (SM) 9.
    Sm9 = 0x0848,
    /// Sync Manager (SM) 10.
    Sm10 = 0x0850,
    /// Sync Manager (SM) 11.
    Sm11 = 0x0858,
    /// Sync Manager (SM) 12.
    Sm12 = 0x0860,
    /// Sync Manager (SM) 13.
    Sm13 = 0x0868,
    /// Sync Manager (SM) 14.
    Sm14 = 0x0870,
    /// Sync Manager (SM) 15.
    Sm15 = 0x0878,

    /// Distributed clock (DC) port 0 receive time in ns.
    ///
    /// Distributed clock registers are defined in ETG1000.4 Table 60.
    DcTimePort0 = 0x0900,
    /// Distributed clock (DC) port 1 receive time in ns.
    DcTimePort1 = 0x0904,
    /// Distributed clock (DC) port 2 receive time in ns.
    DcTimePort2 = 0x0908,
    /// Distributed clock (DC) port 3 receive time in ns.
    DcTimePort3 = 0x090c,
    /// DC system receive time.
    DcReceiveTime = 0x0918,
    /// DC system time.
    DcSystemTime = 0x0910,
    // u64
    /// DC system time offset.
    DcSystemTimeOffset = 0x0920,
    // u32
    /// Transmission delay.
    DcSystemTimeTransmissionDelay = 0x0928,

    // u16
    /// DC control loop parameter.
    DcControlLoopParam1 = 0x0930,
    // u16
    /// DC control loop parameter.
    DcControlLoopParam2 = 0x0932,
    // u16
    /// DC control loop parameter.
    DcControlLoopParam3 = 0x0934,

    // u32
    /// DC system time difference.
    DcSystemTimeDifference = 0x092C,

    /// ETG1000.6 Table 27 – Distributed Clock sync parameter, `u8`.
    ///
    /// AKA ETG1000.4 Table 61 DC user P1.
    DcSyncActive = 0x0981,

    /// ETG1000.6 Table 27 – Distributed Clock sync parameter, `u32`.
    ///
    /// AKA ETG1000.4 Table 61 DC user P4.
    DcSyncStartTime = 0x0990,

    /// ETG1000.6 Table 27 – Distributed Clock sync parameter, `u32`.
    ///
    /// AKA ETG1000.4 Table 61 DC user P5.
    DcSync0CycleTime = 0x09A0,
}

impl From<RegisterAddress> for u16 {
    fn from(reg: RegisterAddress) -> Self {
        reg as u16
    }
}

impl RegisterAddress {
    /// FMMU by index.
    pub fn fmmu(index: u8) -> Self {
        match index {
            0 => Self::Fmmu0,
            1 => Self::Fmmu1,
            2 => Self::Fmmu2,
            3 => Self::Fmmu3,
            4 => Self::Fmmu4,
            5 => Self::Fmmu5,
            6 => Self::Fmmu6,
            7 => Self::Fmmu7,
            8 => Self::Fmmu8,
            9 => Self::Fmmu9,
            10 => Self::Fmmu10,
            11 => Self::Fmmu11,
            12 => Self::Fmmu12,
            13 => Self::Fmmu13,
            14 => Self::Fmmu14,
            15 => Self::Fmmu15,
            index => unreachable!("Bad FMMU index {}", index),
        }
    }

    /// Sync manager by index.
    pub fn sync_manager(index: u8) -> Self {
        match index {
            0 => Self::Sm0,
            1 => Self::Sm1,
            2 => Self::Sm2,
            3 => Self::Sm3,
            4 => Self::Sm4,
            5 => Self::Sm5,
            6 => Self::Sm6,
            7 => Self::Sm7,
            8 => Self::Sm8,
            9 => Self::Sm9,
            10 => Self::Sm10,
            11 => Self::Sm11,
            12 => Self::Sm12,
            13 => Self::Sm13,
            14 => Self::Sm14,
            15 => Self::Sm15,
            index => unreachable!("Bad SM index {}", index),
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

    type Error = WrappedPackingError;

    fn try_from_slice(slice: &[u8]) -> Result<Self, Self::Error> {
        let arr = slice[0..1]
            .try_into()
            .map_err(|_| PackingError::BufferTooSmall)?;

        let res = Self::unpack(arr)?;

        Ok(res)
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

#[derive(Default, Clone, Debug, PartialEq)]
#[cfg_attr(test, derive(arbitrary::Arbitrary))]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct SupportFlags {
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

impl core::fmt::Display for SupportFlags {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        // Just print DC for now
        f.write_str("DC: ")?;

        if self.dc_supported {
            f.write_str("yes")?;

            if self.has_64bit_dc {
                f.write_str(" (64 bit)")?;
            } else {
                f.write_str(" (32 bit)")?;
            }
        } else {
            f.write_str("no")?;
        }

        if self.enhanced_dc_sync {
            f.write_str(", enhanced sync")?;
        }

        Ok(())
    }
}

impl PackedStruct for SupportFlags {
    type ByteArray = [u8; 2];

    fn pack(&self) -> packed_struct::PackingResult<Self::ByteArray> {
        let result = (self.fmmu_supports_bit_ops as u16)
            | (self.reserved_register_support as u16) << 1
            | (self.dc_supported as u16) << 2
            | (self.has_64bit_dc as u16) << 3
            | (self.low_jitter as u16) << 4
            | (self.ebus_enhanced_link_detection as u16) << 5
            | (self.mii_enhanced_link_detection as u16) << 6
            | (self.separate_fcs_error_handling as u16) << 7
            | (self.enhanced_dc_sync as u16) << 8
            | (self.lrw_supported as u16) << 9
            | (self.brw_aprw_fprw_supported as u16) << 10
            | (self.special_fmmu as u16) << 11;

        Ok(result.to_le_bytes())
    }

    fn unpack(src: &Self::ByteArray) -> packed_struct::PackingResult<Self> {
        let raw = u16::from_le_bytes(*src);

        Ok(Self {
            fmmu_supports_bit_ops: (raw & 1) == 1,
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

    type Error = WrappedPackingError;

    fn try_from_slice(slice: &[u8]) -> Result<Self, Self::Error> {
        let res = Self::unpack_from_slice(slice)?;

        Ok(res)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg_attr(miri, ignore)]
    fn support_flags_fuzz() {
        heckcheck::check(|status: SupportFlags| {
            let packed = status.pack().expect("Pack");

            let unpacked = SupportFlags::unpack_from_slice(&packed).expect("Unpack");

            pretty_assertions::assert_eq!(status, unpacked);

            Ok(())
        });
    }
}
