//! Encode/decode error.

/// Wire encode/decode errors.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "defmt-03", derive(defmt::Format))]
pub enum WireError {
    /// TODO!
    Todo,
}

#[cfg(feature = "std")]
impl std::error::Error for WireError {}

impl core::fmt::Display for WireError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("TODO")
    }
}
