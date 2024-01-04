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
pub use ethercrab_wire_derive::EtherCrabWireReadWrite;

/// A type to be received from the wire, according to EtherCAT spec rules (packed bits, little
/// endian).
pub trait EtherCrabWireRead: Sized {
    /// Unpack this type from the beginning of the given buffer.
    fn unpack_from_slice(buf: &[u8]) -> Result<Self, WireError>;

    /// Get the length in bytes of this item when packed.
    fn packed_len(&self) -> usize;
}

/// A type to be sent/received on the wire, according to EtherCAT spec rules (packed bits, little
/// endian).
pub trait EtherCrabWireReadWrite: EtherCrabWireRead {
    /// Pack the type and write it into the beginning of `buf`.
    ///
    /// The default implementation of this method will return an error if the buffer is not long
    /// enough.
    fn pack_to_slice<'buf>(&self, buf: &'buf mut [u8]) -> Result<&'buf [u8], WireError> {
        if buf.len() < self.packed_len() {
            return Err(WireError::Todo);
        }

        Ok(self.pack_to_slice_unchecked(buf))
    }

    /// Pack the type and write it into the beginning of `buf`.
    ///
    /// # Panics
    ///
    /// This method must panic if `buf` is too short to hold the packed data.
    fn pack_to_slice_unchecked<'buf>(&self, buf: &'buf mut [u8]) -> &'buf [u8];
}

/// Implemented for types with a known size at compile time (pretty much everything that isn't a
/// `&[u8]`).
pub trait EtherCrabWireReadWriteSized: EtherCrabWireReadWrite {
    /// Packed size in bytes.
    const PACKED_LEN: usize;

    /// Used to define an array of the correct length. This type should ALWAYS be of the form `[u8;
    /// N]` where `N` is a fixed value or const generic as per the type this trait is implemented
    /// on.
    type Buffer: AsRef<[u8]> + AsMut<[u8]>;

    /// Pack this item to a fixed sized array.
    fn pack(&self) -> Self::Buffer;

    /// Create a buffer sized to contain the packed representation of this item.
    fn buffer() -> Self::Buffer;
}

// // TODO: Figure this out:
// // Bounds should be JUST EtherCrabWireReadWriteSized
// pub trait EtherCrabWireReadOnly {}
// // Bounds should be EtherCrabWireReadOnly + EtherCrabWireReadWrite
// pub trait EtherCrabWireReadWrite {}
