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
}

impl From<BorrowError> for Error {
    fn from(_: BorrowError) -> Self {
        Self::Borrow
    }
}

#[derive(Debug)]
pub enum PduError {
    Timeout,
    IndexInUse,
    Send,
    Decode,
    TooLong,
    CreateFrame(smoltcp::Error),
    Encode(cookie_factory::GenError),
    Address,
    InvalidIndex(usize),
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
