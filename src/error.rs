//! EtherCrab error types.

use crate::{coe::abort_code::AbortCode, command::Command, SlaveState};
use core::{cell::BorrowError, fmt, num::TryFromIntError, str::Utf8Error};

/// An EtherCrab error.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Error {
    /// A low level error occurred when producing or consuming a PDU.
    Pdu(PduError),
    /// A working counter (WKC) error was encountered.
    WorkingCounter {
        /// The expected working counter value.
        expected: u16,
        /// The actual value received.
        received: u16,
        /// Optional context for debugging.
        context: Option<&'static str>,
    },
    /// Failed to borrow an item. This likely points to a race condition.
    Borrow,
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
    /// slaves in the group.
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
    /// There is a problem with the discovered EtherCAT slave topology.
    Topology,
    /// An error was read back from one or more slaves when attempting to transition to a new state.
    StateTransition,
    /// An unknown slave device was encountered during device discovery/initialisation.
    UnknownSlave,
    /// An invalid state was encountered.
    InvalidState {
        /// The desired state.
        expected: SlaveState,

        /// The actual state.
        actual: SlaveState,

        /// Slave address.
        configured_address: u16,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Pdu(e) => write!(f, "pdu: {}", e),
            Error::WorkingCounter {
                expected,
                received,
                context,
            } => write!(
                f,
                "working counter expected {}, got {} ({})",
                expected,
                received,
                context.unwrap_or("[no context provided]")
            ),
            Error::Borrow => write!(f, ""),
            Error::Timeout => write!(f, ""),
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
            Error::SendFrame => write!(f, "failed to send EtherCAT frame"),
            Error::ReceiveFrame => write!(f, "failed to receive an EtherCAT frame"),
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
            Error::Internal => write!(f, "internal error"),
            Error::Topology => write!(f, "topology"),
            Error::StateTransition => write!(f, "a slave failed to transition to a new state"),
            Error::UnknownSlave => write!(f, "unknown slave device"),
            Error::InvalidState {
                expected,
                actual,
                configured_address,
            } => write!(
                f,
                "slave {:#06x} state is invalid: {}, expected {}",
                configured_address, actual, expected
            ),
        }
    }
}

impl From<BorrowError> for Error {
    fn from(_: BorrowError) -> Self {
        Self::Borrow
    }
}

/// The kind of item being looked for.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Item {
    /// An EtherCAT slave device.
    Slave,
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
    /// A user-defined slave group.
    Group,
}

/// Low-level PDU (Process Data Unit) error.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum PduError {
    /// Failed to decode raw PDU data into a given data type.
    Decode,
    /// Something went wrong when encoding/decoding the raw Ethernet II frame.
    Ethernet(smoltcp::Error),
    /// PDU data is too long to fit in the given buffer.
    TooLong,
    /// Failed to create an Ethernet II frame.
    CreateFrame(smoltcp::Error),
    /// A frame index was given that does not point to a frame.
    InvalidIndex(usize),
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

impl fmt::Display for PduError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PduError::Decode => write!(f, "failed to decode raw PDU data into type"),
            PduError::Ethernet(e) => write!(f, "network: {}", e),
            PduError::TooLong => write!(f, "data is too long to fit in given buffer"),
            PduError::CreateFrame(e) => write!(f, "failed to create frame: {}", e),
            PduError::InvalidIndex(index) => write!(f, "invalid PDU index {}", index),
            PduError::Validation(e) => write!(f, "received PDU validation failed: {}", e),
            PduError::InvalidFrameState => write!(f, "invalid PDU frame state"),
            PduError::SwapState => write!(f, "failed to swap frame state"),
        }
    }
}

/// CoE mailbox error.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum MailboxError {
    /// The mailbox operation was aborted.
    Aborted {
        /// Abort code.
        code: AbortCode,
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
    /// A slave has no mailbox but requires one for a given action.
    NoMailbox,
    /// The response to a mailbox action is invalid.
    SdoResponseInvalid {
        /// The address used in the operation.
        address: u16,
        /// The subindex used in the operation.
        sub_index: u8,
    },
}

impl fmt::Display for MailboxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
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
            MailboxError::NoMailbox => write!(f, "device has no mailbox"),
            MailboxError::SdoResponseInvalid { address, sub_index } => write!(
                f,
                "{:#06x}:{} invalid response from device",
                address, sub_index
            ),
        }
    }
}

/// EEPROM (SII) error.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum EepromError {
    /// Failed to decode data from EEPROM.
    Decode,
    /// An EEPROM section is too large to fit in the given buffer.
    SectionOverrun,
    /// The given category does not exist in the slave's EEPROM.
    NoCategory,
    /// The section in the slave's EEPROM is too small to fill the given buffer.
    SectionUnderrun,
}

impl fmt::Display for EepromError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EepromError::Decode => f.write_str("failed to decode data"),
            EepromError::SectionOverrun => f.write_str("section too large to fit in buffer"),
            EepromError::NoCategory => f.write_str("category not found"),
            EepromError::SectionUnderrun => f.write_str("section too short to fill buffer"),
        }
    }
}

/// An EtherCat "visible string" (i.e. a human readable string) error.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum VisibleStringError {
    /// The string could not be decoded.
    Decode(Utf8Error),
    /// The source data is too long to fit in a given storage type.
    TooLong,
}

impl fmt::Display for VisibleStringError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VisibleStringError::Decode(e) => write!(f, "failed to decode string: {}", e),
            VisibleStringError::TooLong => write!(f, "string is too long"),
        }
    }
}

/// A PDU response failed to validate.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
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

impl fmt::Display for PduValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
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

impl From<PduValidationError> for PduError {
    fn from(e: PduValidationError) -> Self {
        Self::Validation(e)
    }
}

impl From<smoltcp::Error> for PduError {
    fn from(e: smoltcp::Error) -> Self {
        Self::Ethernet(e)
    }
}

impl From<smoltcp::Error> for Error {
    fn from(e: smoltcp::Error) -> Self {
        Self::Pdu(e.into())
    }
}

impl<I> From<nom::Err<nom::error::Error<I>>> for Error
where
    I: core::fmt::Debug,
{
    fn from(e: nom::Err<nom::error::Error<I>>) -> Self {
        log::error!("Nom error {:?}", e);

        Self::Pdu(PduError::Decode)
    }
}

impl From<packed_struct::PackingError> for Error {
    fn from(e: packed_struct::PackingError) -> Self {
        log::error!("Packing error {:?}", e);

        Self::Pdu(PduError::Decode)
    }
}

impl From<TryFromIntError> for Error {
    fn from(e: TryFromIntError) -> Self {
        log::error!("Integer conversion error: {}", e);

        Self::IntegerTypeConversion
    }
}
