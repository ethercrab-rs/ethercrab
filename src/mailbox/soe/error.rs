// Table 293: Summary of Service Channel Errors
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum SoeErrorCode {
    // 0x0nnn General error
    // 0x0000 No error in the service channel
    NoError = 0x0000,
    // 0x0001 Service channel not open
    ChannelNotOpen = 0x0001,
    // 0x0009 Invalid access to closing the service channel
    InvalidChannelClosing = 0x0009,
    // 0x1nnn Element 1 (Ident number)
    // 0x1001 No IDN
    NoIDN = 0x1001,
    // 0x1009 Invalid access to element 1
    InvalidAccessToElement1 = 0x1009,
    // 0x2nnn Element 2 (Name)
    // 0x2001 No name
    NoName = 0x2001,
    // 0x2002 Name transmission too short
    NameTransmissionTooShort = 0x2002,
    // 0x2003 Name transmission too long
    NameTransmissionTooLong = 0x2003,
    // 0x2004 Name cannot be changed (read only)
    NameReadOnly = 0x2004,
    // 0x2005 Name is write-protected at this time
    NameWriteProtected = 0x2005,
    // 0x3nnn Element 3 (Attribute)
    // 0x3002 Attribute transmission too short
    AttributeTransmissionTooShort = 0x3002,
    // 0x3003 Attribute transmission too long
    AttributeTransmissionTooLong = 0x3003,
    // 0x3004 Attribute cannot be changed (read only)
    AttributeRreadOnly = 0x3004,
    // 0x3005 Attribute is write-protected at this time
    AttributeWriteProtected = 0x3005,
    // 0x4nnn Element 4 (Unit)
    // 0x4001 No units
    NoUnits = 0x4001,
    // 0x4002 Unit transmission too short
    UnitTransmissionTooShort = 0x4002,
    // 0x4003 Unit transmission too long
    UnitTransmissionTooLong = 0x4003,
    // 0x4004 Unit cannot be changed (read only)
    UnitReadOnly = 0x4004,
    // 0x4005 Unit is write-protected at this time
    UnitWriteProtected = 0x4005,
    // 0x5nnn Element 5 (Minimum input value)
    // 0x5001 No minimum input value
    NoMinimum = 0x5001,
    // 0x5002 Minimum input value transmission too short
    MinimumTransmissionTooShort = 0x5002,
    // 0x5003 Minimum input value transmission too long
    MinimumTransmissionTooLong = 0x5003,
    // 0x5004 Minimum input value cannot be changed (read only)
    MinimumReadOnly = 0x5004,
    // 0x5005 Minimum input value is write-protected at this time
    MinimumWriteProtected = 0x5005,
    // 0x6nnn Element 6 (Maximum input value)
    // 0x6001 No maximum input value
    NoMaximum = 0x6001,
    // 0x6002 Maximum input value transmission too short
    MaximumTransmissionTooShort = 0x6002,
    // 0x6003 Maximum input value transmission too long
    MaximumTransmissionTooLong = 0x6003,
    // 0x6004 Maximum input value cannot be changed (read only)
    MaximumReadOnly = 0x6004,
    // 0x6005 Maximum input value is write-protected at this time
    MaximumWriteProtected = 0x6005,
    // 0x7nnn Element 7 (Operation data)
    // 0x7002 Operation data transmission too short
    OperationDataTransmissionTooShort = 0x7002,
    // 0x7003 Operation data transmission too long
    OperationDataTransmissionTooLong = 0x7003,
    // 0x7004 Operation data is read only
    OperationDataIsReadOnly = 0x7004,
    // 0x7005 Operation data is currently write-protected at this time (e.g. Communication phase)
    OperationDataIsCurrentlyWriteProtected = 0x7005,
    // 0x7006 Operation data is less than the minimum input value
    OperationDataIsLessThanTheMinimum = 0x7006,
    // 0x7007 Operation data exceeds the maximum input value
    OperationDataExceedsTheMaximum = 0x7007,
    // 0x7008 Invalid operation data: Configured IDN will not be supported due to invalid bit number or bit combination
    InvalidOperationData = 0x7008,
    // 0x7009 Operation data is write protected by a password
    OperationDataWriteProtectedByAPassword = 0x7009,
    // 0x700A Operation data is write protected (cyclically configured) (IDN is configured in the MDT or AT. Writing via the service channel is not allowed).
    OperationDataWriteProtectedCyclicallyConfigured = 0x700A,
    // 0x700B Invalid indirect addressing: (e.g., data container, list handling)
    InvalidIndirectAddressing = 0x700B,
    // 0x700C Operation data is write protected, due to other settings. (e.g., parameter, operation mode, drive enable, drive on etc.)
    OperationDataWriteProtectedDueToOtherSettings = 0x700C,
    // 0x700D Invalid floating point number
    InvalidFloatingPointNumber = 0x700D,
    // 0x700E Operation data is write protected at parameterization level
    OperationDataWriteProtectedAtParameterizationLevel = 0x700E,
    // 0x700F Operation data is write protected at operating level
    OperationDataWriteProtectedAtOperatingLevel = 0x700F,
    // 0x7010 Procedure command already active
    ProcedureCommandAlreadyActive = 0x7010,
    // 0x7011 Procedure command not interruptible
    ProcedureCommandNotInterruptible = 0x7011,
    // 0x7012 Procedure command currently not executable (for instance, another communication phase is required to execute the procedure command).
    ProcedureCommandCurrentlyNotExecutable = 0x7012,
    // 0x7013 Procedure command not executable due to either invalid or false parameters
    ProcedureCommandNotExecutableDueToInvalidOrFalseParameters = 0x7013,
    // 0xCnnn vendor-specific sercos error codes
    // 0xC000 unknown internal error
    UnknownInternalError = 0xC000,
    // 0xC001 internal error in IDN task
    InternalErrorInIdnTask = 0xC001,
    // 0xC002 out of memory
    OutOfMemory = 0xC002,
    // 0xC003 packet out of memory
    PacketOutOfMemory = 0xC003,
    // 0xC004 packet done failed
    PacketDoneFailed = 0xC004,
    // 0xC005 send packet failed
    SendPacketFailed = 0xC005,
    // 0xC006 get packet failed
    GetPacketFailed = 0xC006,
    // 0xC007 release packet failed
    ReleasePacketFailed = 0xC007,
    // 0xC008 error during user application transfer e.g. application response not within 5 seconds
    ErrorDuringUserApplicationTransfer = 0xC008,
    Unknown(u16),
}

impl From<u16> for SoeErrorCode {
    fn from(code: u16) -> Self {
        match code {
            0x0000 => Self::NoError,
            0x0001 => Self::ChannelNotOpen,
            0x0009 => Self::InvalidChannelClosing,
            0x1001 => Self::NoIDN,
            0x1009 => Self::InvalidAccessToElement1,
            0x2001 => Self::NoName,
            0x2002 => Self::NameTransmissionTooShort,
            0x2003 => Self::NameTransmissionTooLong,
            0x2004 => Self::NameReadOnly,
            0x2005 => Self::NameWriteProtected,
            0x3002 => Self::AttributeTransmissionTooShort,
            0x3003 => Self::AttributeTransmissionTooLong,
            0x3004 => Self::AttributeRreadOnly,
            0x3005 => Self::AttributeWriteProtected,
            0x4001 => Self::NoUnits,
            0x4002 => Self::UnitTransmissionTooShort,
            0x4003 => Self::UnitTransmissionTooLong,
            0x4004 => Self::UnitReadOnly,
            0x4005 => Self::UnitWriteProtected,
            0x5001 => Self::NoMinimum,
            0x5002 => Self::MinimumTransmissionTooShort,
            0x5003 => Self::MinimumTransmissionTooLong,
            0x5004 => Self::MinimumReadOnly,
            0x5005 => Self::MinimumWriteProtected,
            0x6001 => Self::NoMaximum,
            0x6002 => Self::MaximumTransmissionTooShort,
            0x6003 => Self::MaximumTransmissionTooLong,
            0x6004 => Self::MaximumReadOnly,
            0x6005 => Self::MaximumWriteProtected,
            0x7002 => Self::OperationDataTransmissionTooShort,
            0x7003 => Self::OperationDataTransmissionTooLong,
            0x7004 => Self::OperationDataIsReadOnly,
            0x7005 => Self::OperationDataIsCurrentlyWriteProtected,
            0x7006 => Self::OperationDataIsLessThanTheMinimum,
            0x7007 => Self::OperationDataExceedsTheMaximum,
            0x7008 => Self::InvalidOperationData,
            0x7009 => Self::OperationDataWriteProtectedByAPassword,
            0x700A => Self::OperationDataWriteProtectedCyclicallyConfigured,
            0x700B => Self::InvalidIndirectAddressing,
            0x700C => Self::OperationDataWriteProtectedDueToOtherSettings,
            0x700D => Self::InvalidFloatingPointNumber,
            0x700E => Self::OperationDataWriteProtectedAtParameterizationLevel,
            0x700F => Self::OperationDataWriteProtectedAtOperatingLevel,
            0x7010 => Self::ProcedureCommandAlreadyActive,
            0x7011 => Self::ProcedureCommandNotInterruptible,
            0x7012 => Self::ProcedureCommandCurrentlyNotExecutable,
            0x7013 => Self::ProcedureCommandNotExecutableDueToInvalidOrFalseParameters,
            0xC000 => Self::UnknownInternalError,
            0xC001 => Self::InternalErrorInIdnTask,
            0xC002 => Self::OutOfMemory,
            0xC003 => Self::PacketOutOfMemory,
            0xC004 => Self::PacketDoneFailed,
            0xC005 => Self::SendPacketFailed,
            0xC006 => Self::GetPacketFailed,
            0xC007 => Self::ReleasePacketFailed,
            0xC008 => Self::ErrorDuringUserApplicationTransfer,
            other => Self::Unknown(other),
        }
    }
}
