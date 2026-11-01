#[derive(Clone, Copy, Debug, PartialEq, Eq, ethercrab_wire::EtherCrabWireReadWrite)]
#[cfg_attr(test, derive(arbitrary::Arbitrary))]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[repr(u8)]
pub enum SoeOpcode {
    /// Read Request
    ReadRequest = 0x01,
    /// Read Response
    ReadResponse = 0x02,
    /// Write Request
    WriteRequest = 0x03,
    /// Write Response
    WriteResponse = 0x04,
    /// Notification
    Notification = 0x05,
    /// Emergency / Abort
    Emergency = 0x06,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ethercrab_wire::EtherCrabWireReadWrite)]
#[cfg_attr(test, derive(arbitrary::Arbitrary))]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[repr(u8)]
pub enum SoeFragmentationFlag {
    /// Complete Transmission
    CompleteTransmission = 0,
    /// Incomplete Frame
    IncompleteFrame = 1,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ethercrab_wire::EtherCrabWireReadWrite)]
#[cfg_attr(test, derive(arbitrary::Arbitrary))]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[repr(u8)]
pub enum SoeErrorFlag {
    /// No Error
    NoError = 0,
    /// Error Occurred
    ErrorOccurred = 1,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ethercrab_wire::EtherCrabWireReadWrite)]
#[cfg_attr(test, derive(arbitrary::Arbitrary))]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[repr(u8)]
pub enum SoeElementFlag {
    None = 0b00000000,
    DataStateStatus = 0b00000001,
    NameDescriptor = 0b00000010,
    Attribute = 0b00000100,
    Unit = 0b00001000,
    MinimumValue = 0b00010000,
    MaximumValue = 0b00100000,
    ValueData = 0b01000000,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ethercrab_wire::EtherCrabWireReadWrite)]
#[wire(bytes = 4)]
pub struct SoeHeader {
    // Byte 0 - opcode and flags
    // Bits 0 thru 2 are the opcode
    #[wire(bits = 3)]
    pub opcode: SoeOpcode,
    // Bit 3 is fragmentation
    #[wire(bits = 1)]
    pub fragmentation: SoeFragmentationFlag,
    // Bit 4 is error
    #[wire(bits = 1)]
    pub error: SoeErrorFlag,
    // Bit 5-7 is drive number
    #[wire(bits = 3)]
    pub drive_num: u8,

    // Byte 1 - element flags
    #[wire(bytes = 1)]
    pub element_flag: SoeElementFlag,

    // Bytes 2 & 3 - IDN if fragmentation is not set, number of frames remaining if it is.
    #[wire(bytes = 2)]
    pub idn: u16,
}

// Table 292 of Sercos Master Protocol API, Rev 11
#[derive(Clone, Copy, Debug, PartialEq, Eq, ethercrab_wire::EtherCrabWireReadWrite)]
#[wire(bytes = 4)]
pub struct SoeAttributes {
    #[wire(bytes = 2)]
    pub conversion_factor: u16,

    #[wire(bits = 3)]
    pub data_length: SoeDatalength,

    #[wire(bits = 1)]
    pub is_command: bool,

    #[wire(bits = 3, post_skip = 1)]
    pub display_format: SoeDisplayFormat,

    #[wire(bits = 4)]
    pub decimal_places: u8,

    #[wire(bits = 1)]
    pub write_protect_cp2: bool,
    #[wire(bits = 1)]
    pub write_protect_cp3: bool,
    #[wire(bits = 1, post_skip = 1)]
    pub write_protect_cp4: bool,
}

// Table 292 of Sercos Master Protocol API, Rev 11
#[derive(Clone, Copy, Debug, PartialEq, Eq, ethercrab_wire::EtherCrabWireReadWrite)]
#[cfg_attr(test, derive(arbitrary::Arbitrary))]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[repr(u8)]
pub enum SoeDatalength {
    /// Two Bytes
    TwoBytes = 0b001,
    /// Four Bytes
    FourBytes = 0b010,
    /// Eight Bytes
    EightBytes = 0b011,
    /// Variable 1 Byte Strings
    Variable1ByteStrings = 0b100,
    /// Variable 2 Byte Strings
    Variable2ByteStrings = 0b101,
    /// Variable 4 Byte Strings
    Variable4ByteStrings = 0b110,
    /// Variable 8 Byte Strings
    Variable8ByteStrings = 0b111,
}

impl SoeDatalength {
    pub fn bit_len(self) -> u16 {
        match self {
            SoeDatalength::TwoBytes => 16,
            SoeDatalength::FourBytes => 32,
            SoeDatalength::EightBytes => 64,
            SoeDatalength::Variable1ByteStrings => 0,
            SoeDatalength::Variable2ByteStrings => 0,
            SoeDatalength::Variable4ByteStrings => 0,
            SoeDatalength::Variable8ByteStrings => 0,
        }
    }
}

// Table 292 of Sercos Master Protocol API, Rev 11
#[derive(Clone, Copy, Debug, PartialEq, Eq, ethercrab_wire::EtherCrabWireReadWrite)]
#[cfg_attr(test, derive(arbitrary::Arbitrary))]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[repr(u8)]
pub enum SoeDisplayFormat {
    // 000 - Binary value [Binary]
    Binary = 0b000,
    // 001 - Unsigned integer [Decimal]
    UnsignedIntegerDec = 0b001,
    // 010 - Signed integer [Decimal + sign]
    SignedIntegerDec = 0b010,
    // 011 - Unsigned integer [Hexadecimal]
    UnsignedIntegerHex = 0b011,
    // 100 - Extended character set [Text]
    ExtendedCharacterSet = 0b100,
    // 101 - Unsigned integer [IDN]
    UnsignedIntegerIDN = 0b101,
    // 110 - ANSI 754-1985 floating point number (single precision) [Decimal value with exponent (fraction after decimal point is not taken into account)]
    ANSI754_1985 = 0b110,
    // 111 - SERCOS time[Display format: according to IEC 61588 4 octets seconds & 4 octets nanoseconds, starts with 1.1.1970 computed in UTC]
    SERCOStime = 0b111,
}

// TODO write tests
// #[cfg(test)]
// mod tests {
//     pub use super::*;
//     use ethercrab_wire::{EtherCrabWireRead, EtherCrabWireWriteSized};

//     #[test]
//     fn sanity_soe_service() {
//         assert_eq!(SoeService::SdoRequest.pack(), [0x02]);
//         assert_eq!(
//             SoeService::unpack_from_slice(&[0x02]),
//             Ok(SoeService::SdoRequest)
//         );
//     }
// }
