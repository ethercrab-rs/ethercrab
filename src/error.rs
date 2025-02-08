//! EtherCrab error types.

pub use crate::coe::abort_code::CoeAbortCode;
use crate::{command::Command, fmt, AlStatusCode, SubDeviceState};
use core::num::TryFromIntError;

/// An EtherCrab error.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum Error {
    /// A low level error occurred when producing or consuming a PDU.
    Pdu(PduError),
    /// A working counter (WKC) error was encountered.
    WorkingCounter {
        /// The expected working counter value.
        expected: u16,
        /// The actual value received.
        received: u16,
    },
    /// Something timed out.
    Timeout,
    /// An EEPROM error was encountered.
    Eeprom(EepromError),
    /// A fixed size array was not large enough to hold a given item type.
    Capacity(Item),
    /// A string was too long to fit in a fixed size buffer.
    StringTooLong {
        /// The length of the fixed size buffer.
        max_length: usize,
        /// The length of the input string.
        string_length: usize,
    },
    /// A mailbox error was encountered.
    Mailbox(MailboxError),
    /// Failed to send a frame over the network interace.
    SendFrame,
    /// Failed to receive a frame properly.
    ReceiveFrame,
    /// A frame was only partially sent.
    PartialSend {
        /// Frame length in bytes.
        len: usize,

        /// The number of bytes sent.
        sent: usize,
    },
    /// A value may be too large or otherwise could not be converted into a target type.
    ///
    /// E.g. converting `99_999usize` into a `u16` will fail as the value is larger than `u16::MAX`.
    IntegerTypeConversion,
    /// The allotted storage for a group's PDI is too small for the calculated length read from all
    /// SubDevices in the group.
    PdiTooLong {
        /// Maximum PDI length.
        max_length: usize,

        /// Actual PDI length.
        desired_length: usize,
    },
    /// An item in a list could not be found.
    NotFound {
        /// Item kind.
        item: Item,

        /// An index into a list of items.
        index: Option<usize>,
    },
    /// An internal error occurred. This indicates something that shouldn't happen within EtherCrab.
    Internal,
    /// There is a problem with the discovered EtherCAT SubDevice topology.
    Topology,
    /// An error was read back from one or more SubDevices when attempting to transition to a new
    /// state.
    StateTransition,
    /// An unknown SubDevice was encountered during device discovery/initialisation.
    UnknownSubDevice,
    /// An invalid state was encountered.
    InvalidState {
        /// The desired state.
        expected: SubDeviceState,

        /// The actual state.
        actual: SubDeviceState,

        /// SubDevice address.
        configured_address: u16,
    },

    /// An error occurred encoding or decoding an item.
    Wire(ethercrab_wire::WireError),

    /// A subdevice produced an error.
    SubDevice(AlStatusCode),

    /// A distributed clock error occurred.
    DistributedClock(DistributedClockError),
}

#[cfg(feature = "std")]
impl std::error::Error for Error {}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Error::Pdu(e) => write!(f, "pdu: {}", e),
            Error::WorkingCounter { expected, received } => {
                write!(f, "working counter expected {}, got {}", expected, received)
            }
            Error::Timeout => f.write_str("timeout"),
            Error::Eeprom(e) => write!(f, "eeprom: {}", e),
            Error::Capacity(item) => write!(f, "not enough capacity for {:?}", item),
            Error::StringTooLong {
                max_length,
                string_length,
            } => write!(
                f,
                "string of {} bytes is too long to fit in max storage of {} bytes",
                string_length, max_length
            ),
            Error::Mailbox(e) => write!(f, "mailbox: {e}"),
            Error::SendFrame => f.write_str("failed to send EtherCAT frame"),
            Error::ReceiveFrame => f.write_str("failed to receive an EtherCAT frame"),
            Error::PartialSend { len, sent } => {
                write!(f, "frame of {} bytes only had {} bytes sent", len, sent)
            }
            Error::IntegerTypeConversion => write!(f, "failed to convert between integer types"),
            Error::PdiTooLong {
                max_length,
                desired_length,
            } => write!(
                f,
                "Process Data Image is too long ({} bytes), max length is {}",
                desired_length, max_length
            ),
            Error::NotFound { item, index } => {
                write!(f, "item kind {:?} not found (index: {:?})", item, index)
            }
            Error::Internal => f.write_str("internal error"),
            Error::Topology => f.write_str("topology"),
            Error::StateTransition => {
                f.write_str("a SubDevice failed to transition to a new state")
            }
            Error::UnknownSubDevice => f.write_str("unknown SubDevice"),
            Error::InvalidState {
                expected,
                actual,
                configured_address,
            } => write!(
                f,
                "SubDevice {:#06x} state is invalid: {}, expected {}",
                configured_address, actual, expected
            ),
            Error::Wire(e) => write!(f, "wire encode/decode error: {}", e),
            Error::SubDevice(e) => write!(f, "subdevice error: {}", e),
            Error::DistributedClock(e) => write!(f, "distributed clock: {}", e),
        }
    }
}

/// The kind of item being looked for.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum Item {
    /// An EtherCAT SubDevice.
    SubDevice,
    /// Process Data Object.
    Pdo,
    /// Fieldbus Memory Management Unit.
    Fmmu,
    /// Sync Manager.
    SyncManager,
    /// A PDO entry.
    PdoEntry,
    /// Extended Fieldbus Memory Management Unit config.
    FmmuEx,
    /// A user-defined SubDevice group.
    Group,
}

/// Low-level PDU (Process Data Unit) error.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum PduError {
    /// Failed to decode raw PDU data into a given data type.
    Decode,
    /// Something went wrong when encoding/decoding the raw Ethernet II frame.
    Ethernet,
    /// PDU data is too long to fit in the given buffer.
    TooLong,
    /// Failed to create an Ethernet II frame.
    CreateFrame,
    /// A frame index was given that does not point to a frame.
    InvalidIndex(u8),
    /// A received frame is invalid.
    Validation(PduValidationError),
    /// A frame is not ready to be reused.
    ///
    /// This may be caused by a too small [`MAX_FRAMES`](crate::pdu_loop::PduLoop) value, or sending
    /// frames too quickly.
    InvalidFrameState,
    /// Failed to swap atomic state for a PDU frame.
    ///
    /// This is an internal error and should not appear in user code. Please [open an
    /// issue](https://github.com/ethercrab-rs/ethercrab/issues/new) if this is encountered.
    SwapState,
}

impl core::fmt::Display for PduError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            PduError::Decode => f.write_str("failed to decode raw PDU data into type"),
            PduError::Ethernet => f.write_str("network"),
            PduError::TooLong => f.write_str("data is too long to fit in given buffer"),
            PduError::CreateFrame => f.write_str("failed to create frame"),
            PduError::InvalidIndex(index) => write!(f, "invalid PDU index {}", index),
            PduError::Validation(e) => write!(f, "received PDU validation failed: {}", e),
            PduError::InvalidFrameState => f.write_str("invalid PDU frame state"),
            PduError::SwapState => f.write_str("failed to swap frame state"),
        }
    }
}

/// CoE mailbox error.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum DistributedClockError {
    /// No DC System Time reference SubDevice was found.
    NoReference,
}

impl core::fmt::Display for DistributedClockError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NoReference => f.write_str("No DC reference SubDevice found"),
        }
    }
}

/// CoE mailbox error.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum MailboxError {
    /// The mailbox operation was aborted.
    Aborted {
        /// Abort code.
        code: CoeAbortCode,
        /// The address used in the operation.
        address: u16,
        /// The subindex used in the operation.
        sub_index: u8,
    },
    /// Mailbox data is too long to fit in the given type.
    TooLong {
        /// The address used in the operation.
        address: u16,
        /// The subindex used in the operation.
        sub_index: u8,
    },
    /// A SubDeviceve has no mailbox but requires one for a given action.
    NoMailbox,
    /// The response to a mailbox action is invalid.
    SdoResponseInvalid {
        /// The address used in the operation.
        address: u16,
        /// The subindex used in the operation.
        sub_index: u8,
    },
    /// The returned counter value does not match that which was sent.
    ///
    /// Slowing down mailbox reads may help mitigate this error.
    InvalidCount,
    /// SubDevice sent an emergency message.
    Emergency {
        /// Error code.
        error_code: u16,
        /// Error register.
        error_register: u8,
    },
}

impl core::fmt::Display for MailboxError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            MailboxError::Aborted {
                code,
                address,
                sub_index,
            } => write!(f, "{:#06x}:{} aborted: {}", address, sub_index, code),
            MailboxError::TooLong { address, sub_index } => write!(
                f,
                "{:#06x}:{} returned data is too long",
                address, sub_index
            ),
            MailboxError::NoMailbox => f.write_str("device has no mailbox"),
            MailboxError::SdoResponseInvalid { address, sub_index } => write!(
                f,
                "{:#06x}:{} invalid response from device",
                address, sub_index
            ),
            MailboxError::InvalidCount => f.write_str("incorrect mailbox count value"),
            MailboxError::Emergency {
                error_code,
                error_register,
            } => write!(
                f,
                "emergency: code {:#06x}, register {:#04x}",
                error_code, error_register
            ),
        }
    }
}

/// EEPROM (SII) error.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum EepromError {
    /// Failed to decode data from EEPROM.
    Decode,
    /// An EEPROM section is too large to fit in the given buffer.
    SectionOverrun,
    /// The given category does not exist in the SubDevice's EEPROM.
    NoCategory,
    /// The section in the SubDevice's EEPROM is too small to fill the given buffer.
    SectionUnderrun,
    /// An attempt to clear errors on the device failed.
    ClearErrors,
    /// A general EEPROM error occurred.
    General,
}

impl core::fmt::Display for EepromError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            EepromError::Decode => f.write_str("failed to decode data"),
            EepromError::SectionOverrun => f.write_str("section too large to fit in buffer"),
            EepromError::NoCategory => f.write_str("category not found"),
            EepromError::SectionUnderrun => f.write_str("section too short to fill buffer"),
            EepromError::ClearErrors => f.write_str("clear device errors failed"),
            EepromError::General => f.write_str("general EEPROM error"),
        }
    }
}

/// An EtherCat "visible string" (i.e. a human readable string) error.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum VisibleStringError {
    /// The source data is too long to fit in a given storage type.
    TooLong,
}

#[cfg(feature = "defmt")]
impl defmt::Format for VisibleStringError {
    fn format(&self, f: defmt::Formatter) {
        match self {
            VisibleStringError::TooLong => defmt::write!(f, "TooLong"),
        }
    }
}

impl core::fmt::Display for VisibleStringError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            VisibleStringError::TooLong => f.write_str("string is too long"),
        }
    }
}

/// A PDU response failed to validate.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum PduValidationError {
    /// The index of the received PDU does not match that of the sent one.
    IndexMismatch {
        /// Sent index.
        sent: u8,
        /// Received index.
        received: u8,
    },
    /// The received command does not match the one sent.
    CommandMismatch {
        /// Sent command.
        sent: Command,
        /// Received command.
        received: Command,
    },
}

impl core::fmt::Display for PduValidationError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::IndexMismatch { sent, received } => {
                write!(
                    f,
                    "PDU index mismatch: sent {}, received {}",
                    sent, received
                )
            }
            Self::CommandMismatch { sent, received } => {
                write!(
                    f,
                    "PDU command mismatch: sent {}, received {}",
                    sent, received
                )
            }
        }
    }
}

impl From<PduError> for Error {
    fn from(e: PduError) -> Self {
        Self::Pdu(e)
    }
}

impl From<EepromError> for Error {
    fn from(e: EepromError) -> Self {
        Self::Eeprom(e)
    }
}

impl From<DistributedClockError> for Error {
    fn from(e: DistributedClockError) -> Self {
        Self::DistributedClock(e)
    }
}

impl From<PduValidationError> for PduError {
    fn from(e: PduValidationError) -> Self {
        Self::Validation(e)
    }
}

impl From<TryFromIntError> for Error {
    fn from(_e: TryFromIntError) -> Self {
        fmt::error!("Integer conversion error");

        Self::IntegerTypeConversion
    }
}

impl From<ethercrab_wire::WireError> for Error {
    fn from(value: ethercrab_wire::WireError) -> Self {
        Self::Wire(value)
    }
}
