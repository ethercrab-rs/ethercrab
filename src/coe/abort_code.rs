/// Defined in ETG1000.6 Table 41 – SDO Abort Codes
#[derive(Debug, Copy, Clone, PartialEq, Eq, ethercrab_wire::EtherCrabWireRead)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[repr(u32)]
pub enum CoeAbortCode {
    /// Toggle bit not changed
    ToggleBit = 0x05030000,
    /// SDO protocol timeout
    SdoTimeout = 0x05040000,
    /// Client/Server command specifier not valid or unknown
    InvalidCommand = 0x05040001,
    /// Out of memory
    OutOfMemory = 0x05040005,
    /// Unsupported access to an object
    UnsupportedAccess = 0x06010000,
    /// Attempt to read to a write only object
    WriteOnlyRead = 0x06010001,
    /// Attempt to write to a read only object
    ReadOnlyWrite = 0x06010002,
    /// Subindex cannot be written, SI0 must be 0 for write access
    IndexOnly = 0x06010003,
    /// SDO Complete access not supported for objects of variable length such as ENUM object types
    NoCompleteAccess = 0x06010004,
    /// Object length exceeds mailbox size
    ObjectTooLarge = 0x06010005,
    /// Object mapped to RxPDO, SDO Download blocked
    DownloadBlocked = 0x06010006,
    /// The object does not exist in the object directory
    NotFound = 0x06020000,
    /// The object can not be mapped into the PDO
    PdoMappingFailed = 0x06040041,
    /// The number and length of the objects to be mapped would exceed the PDO length
    PdoTooSmall = 0x06040042,
    /// General parameter incompatibility reason
    Incompatible = 0x06040043,
    /// General internal incompatibility in the device
    Internal = 0x06040047,
    /// Access failed due to a hardware error
    HardwareFailure = 0x06060000,
    /// Data type does not match, length of service parameter does not match
    DataLengthMismatch = 0x06070010,
    /// Data type does not match, length of service parameter too high
    DataTooLong = 0x06070012,
    /// Data type does not match, length of service parameter too low
    DataTooShort = 0x06070013,
    /// Subindex does not exist
    SubIndexNotFound = 0x06090011,
    /// Value range of parameter exceeded (only for write access)
    ValueOutOfRange = 0x06090030,
    /// Value of parameter written too high
    ValueTooLarge = 0x06090031,
    /// Value of parameter written too low
    ValueTooSmall = 0x06090032,
    /// Maximum value is less than minimum value
    MaxMin = 0x06090036,
    /// General error
    General = 0x08000000,

    /// Data cannot be transferred or stored to the application
    ///
    /// NOTE: This is the general Abort Code in case no further detail on the reason can determined.
    /// It is recommended to use one of the more detailed Abort Codes
    /// ([`AbortCode::TransferFailedLocal`], [`AbortCode::InvalidState`])
    TransferFailed = 0x08000020,

    /// Data cannot be transferred or stored to the application because of local control
    ///
    /// NOTE: “local control” means an application specific reason. It does not mean the
    /// ESM-specific control
    TransferFailedLocal = 0x08000021,

    ///  Data cannot be transferred or stored to the application because of the present device state
    ///
    /// NOTE: “device state” means the ESM state
    InvalidState = 0x08000022,

    /// Object dictionary dynamic generation fails or no object dictionary is present
    NoObjectDictionary = 0x08000023,

    /// Unknown abort code.
    #[wire(catch_all)]
    Unknown(u32),
}

impl core::fmt::Display for CoeAbortCode {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let num = u32::from(*self);

        f.write_fmt(format_args!("{:#010x}", num))?;
        f.write_str(": ")?;

        match self {
            Self::ToggleBit => f.write_str("Toggle bit not changed"),
            Self::SdoTimeout => f.write_str("SDO protocol timeout"),
            Self::InvalidCommand => f.write_str("Client/Server command specifier not valid or unknown"),
            Self::OutOfMemory => f.write_str("Out of memory"),
            Self::UnsupportedAccess => f.write_str("Unsupported access to an object"),
            Self::WriteOnlyRead => f.write_str("Attempt to read to a write only object"),
            Self::ReadOnlyWrite => f.write_str("Attempt to write to a read only object"),
            Self::IndexOnly => f.write_str("Subindex cannot be written, SI0 must be 0 for write access"),
            Self::NoCompleteAccess => f.write_str("SDO Complete access not supported for objects of variable length such as ENUM object types"),
            Self::ObjectTooLarge => f.write_str("Object length exceeds mailbox size"),
            Self::DownloadBlocked => f.write_str("Object mapped to RxPDO, SDO Download blocked"),
            Self::NotFound => f.write_str("The object does not exist in the object directory"),
            Self::PdoMappingFailed => f.write_str("The object can not be mapped into the PDO"),
            Self::PdoTooSmall => f.write_str("The number and length of the objects to be mapped would exceed the PDO length"),
            Self::Incompatible => f.write_str("General parameter incompatibility reason"),
            Self::Internal => f.write_str("General internal incompatibility in the device"),
            Self::HardwareFailure => f.write_str("Access failed due to a hardware error"),
            Self::DataLengthMismatch => f.write_str("Data type does not match, length of service parameter does not match"),
            Self::DataTooLong => f.write_str("Data type does not match, length of service parameter too high"),
            Self::DataTooShort => f.write_str("Data type does not match, length of service parameter too low"),
            Self::SubIndexNotFound => f.write_str("Subindex does not exist"),
            Self::ValueOutOfRange => f.write_str("Value range of parameter exceeded (only for write access)"),
            Self::ValueTooLarge => f.write_str("Value of parameter written too high"),
            Self::ValueTooSmall => f.write_str("Value of parameter written too low"),
            Self::MaxMin => f.write_str("Maximum value is less than minimum value"),
            Self::General => f.write_str("General error"),
            Self::TransferFailed => f.write_str("Data cannot be transferred or stored to the application"),
            Self::TransferFailedLocal => f.write_str("Data cannot be transferred or stored to the application because of local control"),
            Self::InvalidState => f.write_str(" Data cannot be transferred or stored to the application because of the present device state"),
            Self::NoObjectDictionary => f.write_str("Object dictionary dynamic generation fails or no object dictionary is present"),
            Self::Unknown(_) => f.write_str("Unknown code"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_code() {
        let code = 0x1234_5678u32;

        let decoded = CoeAbortCode::from(code);

        assert_eq!(decoded, CoeAbortCode::Unknown(0x1234_5678u32));

        assert_eq!(decoded.to_string(), "0x12345678: Unknown code");
    }

    #[test]
    fn parse_code() {
        let code = 0x06090036;

        assert_eq!(CoeAbortCode::from(code), CoeAbortCode::MaxMin);
    }
}
