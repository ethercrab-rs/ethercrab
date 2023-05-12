use crate::pdu_data::*;
use core::fmt;
use packed_struct::PackedStruct;

/// AL (Application Layer) Status Code.
///
/// Defined in ETG1000.6 Table 11.
#[derive(Debug, Copy, Clone, num_enum::TryFromPrimitive, num_enum::IntoPrimitive)]
#[repr(u16)]
pub enum AlStatusCode {
    /// No error
    NoError = 0x0000,
    /// Unspecified error
    UnspecifiedError = 0x0001,
    /// No Memory
    NoMemory = 0x0002,
    /// Invalid Device Setup
    InvalidDeviceSetup = 0x0003,
    /// Reserved due to compatibility reasons
    CompatibilityReserved = 0x0005,
    /// Invalid requested state change
    InvalidRequestedStateChange = 0x0011,
    /// Unknown requested state
    UnknownRequestedState = 0x0012,
    /// Bootstrap not supported
    BootstrapNotSupported = 0x0013,
    /// No valid firmware
    NoValidFirmware = 0x0014,
    /// Invalid mailbox configuration
    InvalidMailboxConfiguration = 0x0015,
    /// Invalid mailbox configuration (second code)
    InvalidMailboxConfiguration2 = 0x0016,
    /// Invalid sync manager configuration
    InvalidSyncManagerConfiguration = 0x0017,
    /// No valid inputs available
    NoValidInputsAvailable = 0x0018,
    /// No valid outputs
    NoValidOutputs = 0x0019,
    /// Synchronization error
    SynchronizationError = 0x001A,
    /// Sync manager watchdog
    SyncManagerWatchdog = 0x001B,
    /// Invalid Sync Manager Types
    InvalidSyncManagerTypes = 0x001C,
    /// Invalid Output Configuration
    InvalidOutputConfiguration = 0x001D,
    /// Invalid Input Configuration
    InvalidInputConfiguration = 0x001E,
    /// Invalid Watchdog Configuration
    InvalidWatchdogConfiguration = 0x001F,
    /// Slave needs cold start
    SlaveNeedsColdStart = 0x0020,
    /// Slave needs INIT
    SlaveNeedsInit = 0x0021,
    /// Slave needs PREOP
    SlaveNeedsPreop = 0x0022,
    /// Slave needs SAFEOP
    SlaveNeedsSafeop = 0x0023,
    /// Invalid Input Mapping
    InvalidInputMapping = 0x0024,
    /// Invalid Output Mapping
    InvalidOutputMapping = 0x0025,
    /// Inconsistent Settings
    InconsistentSettings = 0x0026,
    /// FreeRun not supported
    FreeRunNotSupported = 0x0027,
    /// SyncMode not supported
    SyncModeNotSupported = 0x0028,
    /// FreeRun needs 3 Buffer Mode
    FreeRunNeeds3BufferMode = 0x0029,
    /// Background Watchdog
    BackgroundWatchdog = 0x002A,
    /// No Valid Inputs and Outputs
    NoValidInputsAndOutputs = 0x002B,
    /// Fatal Sync Error
    FatalSyncError = 0x002C,
    /// No Sync Error
    NoSyncError = 0x002D,
    /// Invalid DC SYNC Configuration
    InvalidDcSyncConfiguration = 0x0030,
    /// Invalid DC Latch Configuration
    InvalidDcLatchConfiguration = 0x0031,
    /// PLL Error
    PllError = 0x0032,
    /// DC Sync IO Error
    DcSyncIoError = 0x0033,
    /// DC Sync Timeout Error
    DcSyncTimeoutError = 0x0034,
    /// DC Invalid Sync Cycle Time
    DcInvalidSyncCycleTime = 0x0035,
    /// DC Sync0 Cycle Time
    DcSync0CycleTime = 0x0036,
    /// DC Sync1 Cycle Time
    DcSync1CycleTime = 0x0037,
    /// Mailbox AoE
    MbxAoe = 0x0041,
    /// Mailbox EoE
    MbxEoe = 0x0042,
    /// Mailbox CoE
    MbxCoe = 0x0043,
    /// Mailbox FoE
    MbxFoe = 0x0044,
    /// Mailbox SoE
    MbxSoe = 0x0045,
    /// Mailbox VoE
    MbxVoe = 0x004F,
    /// EEPROM no access
    EepromNoAccess = 0x0050,
    /// EEPROM Error
    EepromError = 0x0051,
    /// Slave restarted locally
    SlaveRestartedLocally = 0x0060,
    /// Device Identification value updated
    DeviceIdentificationValueUpdated = 0x0061,
    /// Application controller available
    ApplicationControllerAvailableI = 0x00F0,
    // NOTE: Other codes < 0x8000 are reserved.
    // NOTE: Codes 0x8000 - 0xffff are vendor specific.
}

impl PduStruct for AlStatusCode {}
impl PackedStruct for AlStatusCode {
	type ByteArray = [u8; 2];
    fn pack(&self) -> PackingResult<Self::ByteArray>  {
		Ok( (*self as u16).to_le_bytes() )
	}
    fn unpack(src: &Self::ByteArray) -> PackingResult<Self>  {
		Self::try_from( u16::from_le_bytes(src.clone()) )
			.map_err(|_|  PackingError::BitsError)
	}
}

impl fmt::Display for AlStatusCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            AlStatusCode::NoError => "0x0000: No error",
            AlStatusCode::UnspecifiedError => "0x0001: Unspecified error",
            AlStatusCode::NoMemory => "0x0002: No Memory",
            AlStatusCode::InvalidDeviceSetup => "0x0003: Invalid Device Setup",
            AlStatusCode::CompatibilityReserved => "0x0005: Reserved due to compatibility reasons",
            AlStatusCode::InvalidRequestedStateChange => "0x0011: Invalid requested state change",
            AlStatusCode::UnknownRequestedState => "0x0012: Unknown requested state",
            AlStatusCode::BootstrapNotSupported => "0x0013: Bootstrap not supported",
            AlStatusCode::NoValidFirmware => "0x0014: No valid firmware",
            AlStatusCode::InvalidMailboxConfiguration => "0x0015: Invalid mailbox configuration",
            AlStatusCode::InvalidMailboxConfiguration2 => "0x0016: Invalid mailbox configuration",
            AlStatusCode::InvalidSyncManagerConfiguration => {
                "0x0017: Invalid sync manager configuration"
            }
            AlStatusCode::NoValidInputsAvailable => "0x0018: No valid inputs available",
            AlStatusCode::NoValidOutputs => "0x0019: No valid outputs",
            AlStatusCode::SynchronizationError => "0x001A: Synchronization error",
            AlStatusCode::SyncManagerWatchdog => "0x001B: Sync manager watchdog",
            AlStatusCode::InvalidSyncManagerTypes => "0x001C: Invalid Sync Manager Types",
            AlStatusCode::InvalidOutputConfiguration => "0x001D: Invalid Output Configuration",
            AlStatusCode::InvalidInputConfiguration => "0x001E: Invalid Input Configuration",
            AlStatusCode::InvalidWatchdogConfiguration => "0x001F: Invalid Watchdog Configuration",
            AlStatusCode::SlaveNeedsColdStart => "0x0020: Slave needs cold start",
            AlStatusCode::SlaveNeedsInit => "0x0021: Slave needs INIT",
            AlStatusCode::SlaveNeedsPreop => "0x0022: Slave needs PREOP",
            AlStatusCode::SlaveNeedsSafeop => "0x0023: Slave needs SAFEOP",
            AlStatusCode::InvalidInputMapping => "0x0024: Invalid Input Mapping",
            AlStatusCode::InvalidOutputMapping => "0x0025: Invalid Output Mapping",
            AlStatusCode::InconsistentSettings => "0x0026: Inconsistent Settings",
            AlStatusCode::FreeRunNotSupported => "0x0027: FreeRun not supported",
            AlStatusCode::SyncModeNotSupported => "0x0028: SyncMode not supported",
            AlStatusCode::FreeRunNeeds3BufferMode => "0x0029: FreeRun needs 3 Buffer Mode",
            AlStatusCode::BackgroundWatchdog => "0x002A: Background Watchdog",
            AlStatusCode::NoValidInputsAndOutputs => "0x002B: No Valid Inputs and Outputs",
            AlStatusCode::FatalSyncError => "0x002C: Fatal Sync Error",
            AlStatusCode::NoSyncError => "0x002D: No Sync Error",
            AlStatusCode::InvalidDcSyncConfiguration => "0x0030: Invalid DC SYNC Configuration",
            AlStatusCode::InvalidDcLatchConfiguration => "0x0031: Invalid DC Latch Configuration",
            AlStatusCode::PllError => "0x0032: PLL Error",
            AlStatusCode::DcSyncIoError => "0x0033: DC Sync IO Error",
            AlStatusCode::DcSyncTimeoutError => "0x0034: DC Sync Timeout Error",
            AlStatusCode::DcInvalidSyncCycleTime => "0x0035: DC Invalid Sync Cycle Time",
            AlStatusCode::DcSync0CycleTime => "0x0036: DC Sync0 Cycle Time",
            AlStatusCode::DcSync1CycleTime => "0x0037: DC Sync1 Cycle Time",
            AlStatusCode::MbxAoe => "0x0041: Mailbox AoE",
            AlStatusCode::MbxEoe => "0x0042: Mailbox EoE",
            AlStatusCode::MbxCoe => "0x0043: Mailbox CoE",
            AlStatusCode::MbxFoe => "0x0044: Mailbox FoE",
            AlStatusCode::MbxSoe => "0x0045: Mailbox SoE",
            AlStatusCode::MbxVoe => "0x004F: Mailbox VoE",
            AlStatusCode::EepromNoAccess => "0x0050: EEPROM no access",
            AlStatusCode::EepromError => "0x0051: EEPROM Error",
            AlStatusCode::SlaveRestartedLocally => "0x0060: Slave restarted locally",
            AlStatusCode::DeviceIdentificationValueUpdated => {
                "0x0061: Device Identification value updated"
            }
            AlStatusCode::ApplicationControllerAvailableI => {
                "0x00F0: Application controller available"
            }
        };

        f.write_str(s)
    }
}
