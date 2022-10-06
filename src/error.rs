use core::cell::BorrowError;

use crate::command::Command;

#[derive(Debug)]
pub enum Error {
    Pdu(PduError),
    WorkingCounter {
        expected: u16,
        received: u16,
        context: Option<&'static str>,
    },
    /// There is not enough storage to hold the number of detected slaves.
    TooManySlaves,
    /// Failed to borrow an item. This likely points to a race condition.
    Borrow,
    /// Slave index not found.
    SlaveNotFound(usize),
    // TODO: Remove from PduError
    Timeout,

    // TODO: Nested enum for more EEPROM failure states.
    EepromDecode,
    EepromSectionOverrun,
    EepromNoCategory,
    EepromSectionUnderrun,
    /// A fixed size array was not large enough to hold a given item.
    Capacity(Capacity),
    Other,
}

impl From<BorrowError> for Error {
    fn from(_: BorrowError) -> Self {
        Self::Borrow
    }
}

#[derive(Debug)]
pub enum Capacity {
    Pdo,
    Fmmu,
    SyncManager,
    PdoEntry,
    FmmuEx,
}

#[derive(Debug)]
pub enum PduError {
    Timeout,
    /// A frame index is currently in use.
    ///
    /// This is caused by an index wraparound in the frame sending buffer. Either reduce the rate at
    /// which frames are sent, speed up frame response processing, or increase the length of the
    /// frame buffer.
    IndexInUse,
    Send,
    /// Failed to decode raw PDU data into a given data type.
    Decode,
    Ethernet(smoltcp::Error),
    /// PDU data is too long to fit in the given array.
    TooLong,
    CreateFrame(smoltcp::Error),
    Encode(cookie_factory::GenError),
    Address,
    InvalidIndex(u8),
    Validation(PduValidationError),
    Parse,
    InvalidFrameState,
}

#[derive(Copy, Clone, Debug)]
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

impl<I> From<nom::Err<nom::error::Error<I>>> for Error {
    fn from(_: nom::Err<nom::error::Error<I>>) -> Self {
        Self::Pdu(PduError::Decode)
    }
}
