//! Traits used to pack/unpack structs and enums from EtherCAT packets on the wire.
//!
//! # Experimental
//!
//! This crate is currently minimal and very rough as it is only used internally by
//! [`ethercrab`](https://crates.io/crates/ethercrab). It is not recommended for public use (yet)
//! and may change at any time.

#![cfg_attr(not(feature = "std"), no_std)]
#![deny(missing_docs)]
#![deny(missing_copy_implementations)]
#![deny(trivial_casts)]
#![deny(trivial_numeric_casts)]
#![deny(unused_import_braces)]
#![deny(unused_qualifications)]
#![deny(rustdoc::broken_intra_doc_links)]
#![deny(rustdoc::private_intra_doc_links)]

mod error;
mod impls;

pub use error::WireError;
pub use ethercrab_wire_derive::{EtherCrabWireRead, EtherCrabWireReadWrite, EtherCrabWireWrite};

/// A type to be received from the wire, according to EtherCAT spec rules (packed bits, little
/// endian).
pub trait EtherCrabWireRead: Sized {
    /// Unpack this type from the beginning of the given buffer.
    fn unpack_from_slice(buf: &[u8]) -> Result<Self, WireError>;
}

/// A type to be sent/received on the wire, according to EtherCAT spec rules (packed bits, little
/// endian).
pub trait EtherCrabWireWrite {
    /// Pack the type and write it into the beginning of `buf`.
    ///
    /// The default implementation of this method will return an error if the buffer is not long
    /// enough.
    fn pack_to_slice<'buf>(&self, buf: &'buf mut [u8]) -> Result<&'buf [u8], WireError> {
        if buf.len() < self.packed_len() {
            return Err(WireError::WriteBufferTooShort {
                expected: self.packed_len(),
                got: buf.len(),
            });
        }

        Ok(self.pack_to_slice_unchecked(buf))
    }

    /// Pack the type and write it into the beginning of `buf`.
    ///
    /// # Panics
    ///
    /// This method must panic if `buf` is too short to hold the packed data.
    fn pack_to_slice_unchecked<'buf>(&self, buf: &'buf mut [u8]) -> &'buf [u8];

    /// Get the length in bytes of this item when packed.
    fn packed_len(&self) -> usize;
}

/// A type that can be both written to the wire and read back from it.
pub trait EtherCrabWireReadWrite: EtherCrabWireRead + EtherCrabWireWrite {}

impl<T> EtherCrabWireReadWrite for T where T: EtherCrabWireRead + EtherCrabWireWrite {}

/// Implemented for types with a known size at compile time.
pub trait EtherCrabWireSized {
    /// Packed size in bytes.
    const PACKED_LEN: usize;

    /// Used to define an array of the correct length. This type should generlaly be of the form
    /// `[u8; N]` where `N` is a fixed value or const generic as per the type this trait is
    /// implemented on.
    type Buffer: AsRef<[u8]> + AsMut<[u8]>;

    /// Create a buffer sized to contain the packed representation of this item.
    fn buffer() -> Self::Buffer;
}

/// Implemented for writeable types with a known size at compile time.
pub trait EtherCrabWireWriteSized: EtherCrabWireSized {
    /// Pack this item to a fixed sized array.
    fn pack(&self) -> Self::Buffer;
}

/// A readable type that has a size known at compile time.
pub trait EtherCrabWireReadSized: EtherCrabWireRead + EtherCrabWireSized {}

impl<T> EtherCrabWireReadSized for T where T: EtherCrabWireRead + EtherCrabWireSized {}
