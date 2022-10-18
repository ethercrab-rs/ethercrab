use crate::{coe::abort_code::AbortCode, command::Command};
use core::{cell::BorrowError, num::TryFromIntError, str::Utf8Error};

#[derive(Debug)]
pub enum Error {
    Pdu(PduError),
    WorkingCounter {
        expected: u16,
        received: u16,
        context: Option<&'static str>,
    },
    /// Failed to borrow an item. This likely points to a race condition.
    Borrow,
    Timeout,
    Eeprom(EepromError),
    /// A fixed size array was not large enough to hold a given item type.
    Capacity(Item),
    StringTooLong {
        desired: usize,
        required: usize,
    },
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
        desired: usize,
        required: usize,
    },
    /// An item in a list could not be found.
    NotFound {
        item: Item,
        index: Option<usize>,
    },
}

impl From<BorrowError> for Error {
    fn from(_: BorrowError) -> Self {
        Self::Borrow
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Item {
    /// There is not enough storage to hold the number of detected slaves.
    Slave,
    Pdo,
    Fmmu,
    SyncManager,
    PdoEntry,
    FmmuEx,
    Group,
}

#[derive(Debug)]
pub enum PduError {
    /// Failed to decode raw PDU data into a given data type.
    Decode,
    Ethernet(smoltcp::Error),
    /// PDU data is too long to fit in the given array.
    TooLong,
    CreateFrame(smoltcp::Error),
    Encode(cookie_factory::GenError),
    InvalidIndex(usize),
    Validation(PduValidationError),
    /// A frame is not ready to be reused.
    ///
    /// This may be caused by a too small [`MAX_FRAMES`](crate::pdu_loop::PduLoop) value, or sending frames too quickly.
    InvalidFrameState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MailboxError {
    Aborted(AbortCode),
    /// Mailbox data is too long to fit in the given type.
    TooLong,
    /// A slave has no mailbox but requires one for a given action.
    NoMailbox,
    SdoResponseInvalid,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EepromError {
    Decode,
    SectionOverrun,
    NoCategory,
    SectionUnderrun,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VisibleStringError {
    Decode(Utf8Error),
    TooLong,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum PduValidationError {
    IndexMismatch { sent: Command, received: Command },
    CommandMismatch { sent: Command, received: Command },
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
