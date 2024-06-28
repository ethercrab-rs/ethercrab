//! Traits used to pack/unpack structs and enums from EtherCAT packets on the wire.
//!
//! This crate is designed for use with [`ethercrab`](https://docs.rs/ethercrab) but can be
//! used standalone too.
//!
//! While these traits can be implemented by hand as normal, it is recommended to derive them using
//! [`ethercrab-wire-derive`](https://docs.rs/ethercrab-wire-derive) where possible.
//!
//! # Experimental
//!
//! This crate is in its early stages and may contain bugs or publish breaking changes at any time.
//! It is in use by [`ethercrab`](https://docs.rs/ethercrab) and is well exercised there,
//! but please use with caution in your own code.

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
///
/// This trait is [derivable](https://docs.rs/ethercrab-wire-derive).
pub trait EtherCrabWireRead: Sized {
    /// Unpack this type from the beginning of the given buffer.
    fn unpack_from_slice(buf: &[u8]) -> Result<Self, WireError>;
}

/// A type to be sent/received on the wire, according to EtherCAT spec rules (packed bits, little
/// endian).
///
/// This trait is [derivable](https://docs.rs/ethercrab-wire-derive).
pub trait EtherCrabWireWrite {
    /// Pack the type and write it into the beginning of `buf`.
    ///
    /// The default implementation of this method will return an error if the buffer is not long
    /// enough.
    fn pack_to_slice<'buf>(&self, buf: &'buf mut [u8]) -> Result<&'buf [u8], WireError> {
        buf.get(0..self.packed_len())
            .ok_or(WireError::WriteBufferTooShort)?;

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
///
/// This trait is [derivable](https://docs.rs/ethercrab-wire-derive).
pub trait EtherCrabWireReadWrite: EtherCrabWireRead + EtherCrabWireWrite {}

impl<T> EtherCrabWireReadWrite for T where T: EtherCrabWireRead + EtherCrabWireWrite {}

/// Implemented for types with a known size at compile time.
///
/// This trait is implemented automatically if [`EtherCrabWireRead`], [`EtherCrabWireWrite`] or
/// [`EtherCrabWireReadWrite`] is [derived](https://docs.rs/ethercrab-wire-derive).
pub trait EtherCrabWireSized {
    /// Packed size in bytes.
    const PACKED_LEN: usize;

    /// Used to define an array of the correct length. This type should be an array `[u8; N]` where
    /// `N` is a fixed value or const generic as per the type this trait is implemented on.
    type Buffer: AsRef<[u8]> + AsMut<[u8]>;

    /// Create a buffer sized to contain the packed representation of this item.
    fn buffer() -> Self::Buffer;
}

/// Implemented for writeable types with a known size at compile time.
///
/// This trait is implemented automatically if [`EtherCrabWireWrite`] or [`EtherCrabWireReadWrite`]
/// is [derived](https://docs.rs/ethercrab-wire-derive).
pub trait EtherCrabWireWriteSized: EtherCrabWireSized {
    /// Pack this item to a fixed sized array.
    fn pack(&self) -> Self::Buffer;
}

/// A readable type that has a size known at compile time.
///
/// This trait is [derivable](https://docs.rs/ethercrab-wire-derive).
pub trait EtherCrabWireReadSized: EtherCrabWireRead + EtherCrabWireSized {}

impl<T> EtherCrabWireReadSized for T where T: EtherCrabWireRead + EtherCrabWireSized {}
