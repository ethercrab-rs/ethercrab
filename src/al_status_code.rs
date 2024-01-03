use crate::fmt;
use crate::pdu_data::PduRead;

/// AL (Application Layer) Status Code.
///
/// Defined in ETG1000.6 Table 11.
#[derive(Debug, Copy, Clone, ethercrab_wire::EtherCatWire)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
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
    ApplicationControllerAvailable = 0x00F0,
    // NOTE: Other codes < 0x8000 are reserved.
    // NOTE: Codes 0x8000 - 0xffff are vendor specific.
    /// Unknown status code.
    #[wire(catch_all)]
    Unknown(u16),
}

impl PduRead for AlStatusCode {
    const LEN: u16 = u16::LEN;

    type Error = core::convert::Infallible;

    fn try_from_slice(slice: &[u8]) -> Result<Self, Self::Error> {
        let data = u16::from_le_bytes(fmt::unwrap!(slice.try_into()));

        Ok(Self::from(data))
    }
}

impl core::fmt::Display for AlStatusCode {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let num = u16::from(*self);

        f.write_fmt(format_args!("{:#06x}", num))?;
        f.write_str(": ")?;

        let s = match self {
            AlStatusCode::NoError => "No error",
            AlStatusCode::UnspecifiedError => "Unspecified error",
            AlStatusCode::NoMemory => "No Memory",
            AlStatusCode::InvalidDeviceSetup => "Invalid Device Setup",
            AlStatusCode::CompatibilityReserved => "Reserved due to compatibility reasons",
            AlStatusCode::InvalidRequestedStateChange => "Invalid requested state change",
            AlStatusCode::UnknownRequestedState => "Unknown requested state",
            AlStatusCode::BootstrapNotSupported => "Bootstrap not supported",
            AlStatusCode::NoValidFirmware => "No valid firmware",
            AlStatusCode::InvalidMailboxConfiguration => "Invalid mailbox configuration",
            AlStatusCode::InvalidMailboxConfiguration2 => "Invalid mailbox configuration",
            AlStatusCode::InvalidSyncManagerConfiguration => "Invalid sync manager configuration",
            AlStatusCode::NoValidInputsAvailable => "No valid inputs available",
            AlStatusCode::NoValidOutputs => "No valid outputs",
            AlStatusCode::SynchronizationError => "Synchronization error",
            AlStatusCode::SyncManagerWatchdog => "Sync manager watchdog",
            AlStatusCode::InvalidSyncManagerTypes => "Invalid Sync Manager Types",
            AlStatusCode::InvalidOutputConfiguration => "Invalid Output Configuration",
            AlStatusCode::InvalidInputConfiguration => "Invalid Input Configuration",
            AlStatusCode::InvalidWatchdogConfiguration => "Invalid Watchdog Configuration",
            AlStatusCode::SlaveNeedsColdStart => "Slave needs cold start",
            AlStatusCode::SlaveNeedsInit => "Slave needs INIT",
            AlStatusCode::SlaveNeedsPreop => "Slave needs PREOP",
            AlStatusCode::SlaveNeedsSafeop => "Slave needs SAFEOP",
            AlStatusCode::InvalidInputMapping => "Invalid Input Mapping",
            AlStatusCode::InvalidOutputMapping => "Invalid Output Mapping",
            AlStatusCode::InconsistentSettings => "Inconsistent Settings",
            AlStatusCode::FreeRunNotSupported => "FreeRun not supported",
            AlStatusCode::SyncModeNotSupported => "SyncMode not supported",
            AlStatusCode::FreeRunNeeds3BufferMode => "FreeRun needs 3 Buffer Mode",
            AlStatusCode::BackgroundWatchdog => "Background Watchdog",
            AlStatusCode::NoValidInputsAndOutputs => "No Valid Inputs and Outputs",
            AlStatusCode::FatalSyncError => "Fatal Sync Error",
            AlStatusCode::NoSyncError => "No Sync Error",
            AlStatusCode::InvalidDcSyncConfiguration => "Invalid DC SYNC Configuration",
            AlStatusCode::InvalidDcLatchConfiguration => "Invalid DC Latch Configuration",
            AlStatusCode::PllError => "PLL Error",
            AlStatusCode::DcSyncIoError => "DC Sync IO Error",
            AlStatusCode::DcSyncTimeoutError => "DC Sync Timeout Error",
            AlStatusCode::DcInvalidSyncCycleTime => "DC Invalid Sync Cycle Time",
            AlStatusCode::DcSync0CycleTime => "DC Sync0 Cycle Time",
            AlStatusCode::DcSync1CycleTime => "DC Sync1 Cycle Time",
            AlStatusCode::MbxAoe => "Mailbox AoE",
            AlStatusCode::MbxEoe => "Mailbox EoE",
            AlStatusCode::MbxCoe => "Mailbox CoE",
            AlStatusCode::MbxFoe => "Mailbox FoE",
            AlStatusCode::MbxSoe => "Mailbox SoE",
            AlStatusCode::MbxVoe => "Mailbox VoE",
            AlStatusCode::EepromNoAccess => "EEPROM no access",
            AlStatusCode::EepromError => "EEPROM Error",
            AlStatusCode::SlaveRestartedLocally => "Slave restarted locally",
            AlStatusCode::DeviceIdentificationValueUpdated => "Device Identification value updated",
            AlStatusCode::ApplicationControllerAvailable => "Application controller available",
            AlStatusCode::Unknown(_) => "(unknown)",
        };

        f.write_str(s)
    }
}
