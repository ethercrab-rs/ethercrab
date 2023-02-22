//! EtherCrab error types.

use crate::{coe::abort_code::AbortCode, command::Command, SlaveState};
use core::{cell::BorrowError, num::TryFromIntError, str::Utf8Error};

/// An EtherCrab error.
#[derive(Debug)]
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

        /// An optional slave address. If this is `None`, the state represents the network as a
        /// whole.
        configured_address: Option<u16>,
    },
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
#[derive(Debug)]
pub enum PduError {
    /// Failed to decode raw PDU data into a given data type.
    Decode,
    /// Something went wrong when encoding/decoding the raw Ethernet II frame.
    Ethernet(smoltcp::Error),
    /// PDU data is too long to fit in the given array.
    TooLong,
    /// Failed to create an Ethernet II frame.
    CreateFrame(smoltcp::Error),
    /// Failed to encode one or more values into raw byte data.
    Encode(cookie_factory::GenError),
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
    SwapState,
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

/// An EtherCat "visible string" (i.e. a human readable string) error.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum VisibleStringError {
    /// The string could not be decoded.
    Decode(Utf8Error),
    /// The source data is too long to fit in a given storage type.
    TooLong,
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

impl From<cookie_factory::GenError> for PduError {
    fn from(e: cookie_factory::GenError) -> Self {
        Self::Encode(e)
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
