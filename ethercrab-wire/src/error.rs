//! Encode/decode error.

/// Wire encode/decode errors.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "defmt-03", derive(defmt::Format))]
pub enum WireError {
    /// The buffer to extract a type from is too short to do so.
    ReadBufferTooShort {
        /// Minimum required buffer length.
        expected: usize,
        /// Actual length given.
        got: usize,
    },
    /// The buffer to write the packed data into is too short.
    WriteBufferTooShort {
        /// Minimum required buffer length.
        expected: usize,
        /// Actual length given.
        got: usize,
    },
    /// Invalid enum or struct value.
    ///
    /// If this comes from an enum, consider adding a variant with `#[wire(catch_all)]` or
    /// `#[wire(alternatives = [])]`.
    InvalidValue,
    /// Failed to create an array of the correct length.
    ArrayLength,
}

#[cfg(feature = "std")]
impl std::error::Error for WireError {}

impl core::fmt::Display for WireError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            WireError::ReadBufferTooShort { expected, got } => write!(
                f,
                "Need at least {} read buffer bytes, got {}",
                expected, got
            ),
            WireError::WriteBufferTooShort { expected, got } => write!(
                f,
                "Need at least {} write buffer bytes, got {}",
                expected, got
            ),
            WireError::InvalidValue => f.write_str("Invalid decoded value"),
            WireError::ArrayLength => f.write_str("Incorrect array length"),
        }
    }
}
