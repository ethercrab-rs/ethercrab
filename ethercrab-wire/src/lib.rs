//! Traits used to pack/unpack structs and enums from EtherCAT packets on the wire.
//!
//! This crate is currently minimal and very rough as it is only used internally by
//! [`ethercrab`](https://crates.io/crates/ethercrab). It is not recommended for public use (yet)
//! and may change at any time.

// TODO: Can we get rid of PduData and PduRead with these traits?

#![cfg_attr(not(feature = "std"), no_std)]
#![deny(missing_docs)]
#![deny(missing_copy_implementations)]
#![deny(trivial_casts)]
#![deny(trivial_numeric_casts)]
#![deny(unused_import_braces)]
#![deny(unused_qualifications)]
#![deny(rustdoc::broken_intra_doc_links)]
#![deny(rustdoc::private_intra_doc_links)]

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

macro_rules! impl_primitive_wire_field {
    ($ty:ty, $size:expr) => {
        impl EtherCatWire for $ty {
            const BYTES: usize = $size;

            type Arr = [u8; $size];

            fn pack_to_slice_unchecked<'buf>(&self, buf: &'buf mut [u8]) -> &'buf [u8] {
                let chunk = &mut buf[0..Self::BYTES];

                chunk.copy_from_slice(&self.to_le_bytes());

                chunk
            }

            fn unpack_from_slice(buf: &[u8]) -> Result<Self, WireError> {
                buf.get(0..Self::BYTES)
                    .ok_or(WireError::Todo)
                    .and_then(|raw| raw.try_into().map_err(|_| WireError::Todo))
                    .map(Self::from_le_bytes)
            }

            fn pack(&self) -> Self::Arr {
                self.to_le_bytes()
            }
        }
    };
}

impl_primitive_wire_field!(u8, 1);
impl_primitive_wire_field!(u16, 2);
impl_primitive_wire_field!(u32, 4);

impl EtherCatWire for bool {
    const BYTES: usize = 1;

    type Arr = [u8; 1];

    fn pack_to_slice_unchecked<'buf>(&self, buf: &'buf mut [u8]) -> &'buf [u8] {
        buf[0] = *self as u8;

        &buf[0..1]
    }

    fn unpack_from_slice(buf: &[u8]) -> Result<Self, WireError> {
        if buf.is_empty() {
            return Err(WireError::Todo);
        }

        Ok(buf[0] == 1)
    }

    fn pack(&self) -> Self::Arr {
        [*self as u8; 1]
    }
}

impl<const N: usize> EtherCatWire for [u8; N] {
    const BYTES: usize = N;

    type Arr = [u8; N];

    fn pack_to_slice_unchecked<'buf>(&self, buf: &'buf mut [u8]) -> &'buf [u8] {
        let buf = &mut buf[0..N];

        buf.copy_from_slice(self);

        buf
    }

    fn unpack_from_slice(buf: &[u8]) -> Result<Self, WireError> {
        let chunk = buf.get(0..N).ok_or(WireError::Todo)?;

        chunk.try_into().map_err(|_e| WireError::Todo)
    }

    fn pack(&self) -> Self::Arr {
        *self
    }
}

/// A type to be sent/received on the wire, according to EtherCAT spec rules (packed bits, little
/// endian).
pub trait EtherCatWire: Sized {
    /// The number of bytes rounded up that can hold this type.
    const BYTES: usize;

    /// Used to define an array of the correct length. This type should ALWAYS be of the form `[u8;
    /// N]` where `N` is a fixed value or const generic as per the type this trait is implemented
    /// on.
    type Arr;

    /// Pack the type and write it into the beginning of `buf`.
    ///
    /// The default implementation of this method will return an error if the buffer is not long
    /// enough.
    fn pack_to_slice<'buf>(&self, buf: &'buf mut [u8]) -> Result<&'buf [u8], WireError> {
        if buf.len() < Self::BYTES {
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

    /// Pack this item to a fixed sized array.
    fn pack(&self) -> Self::Arr;

    /// Unpack this type from the beginning of the given buffer.
    fn unpack_from_slice(buf: &[u8]) -> Result<Self, WireError>;

    // DELETEME?
    /// Get the length in bytes of this item when packed.
    fn packed_len(&self) -> usize {
        Self::BYTES
    }
}

pub use ethercrab_wire_derive::EtherCatWire;
