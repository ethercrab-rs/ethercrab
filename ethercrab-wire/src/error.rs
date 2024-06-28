//! Encode/decode error.

/// Wire encode/decode errors.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "defmt-03", derive(defmt::Format))]
pub enum WireError {
    /// The buffer to extract a type from is too short to do so.
    ReadBufferTooShort,
    /// The buffer to write the packed data into is too short.
    WriteBufferTooShort,
    /// Invalid enum or struct value.
    ///
    /// If this comes from an enum, consider adding a variant with `#[wire(catch_all)]` or
    /// `#[wire(alternatives = [])]`.
    InvalidValue,
    /// Failed to create an array of the correct length.
    ArrayLength,
    /// Valid UTF8 input data is required to decode to a string.
    InvalidUtf8,
}

#[cfg(feature = "std")]
impl std::error::Error for WireError {}

impl core::fmt::Display for WireError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            WireError::ReadBufferTooShort => {
                write!(f, "Read buffer too short to extract type from")
            }
            WireError::WriteBufferTooShort => {
                write!(f, "Write buffer too short to pack type into")
            }
            WireError::InvalidValue => f.write_str("Invalid decoded value"),
            WireError::ArrayLength => f.write_str("Incorrect array length"),
            WireError::InvalidUtf8 => f.write_str("Invalid UTF8"),
        }
    }
}
