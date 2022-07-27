#![cfg_attr(not(feature = "std"), no_std)]

pub mod al_status;
pub mod al_status_code;
pub mod client;
pub mod client_inner;
pub mod command;
pub mod error;
pub mod fmmu;
pub mod frame;
pub mod pdu;
pub mod register;
pub mod slave;
pub mod sync_manager_channel;
pub mod timer_factory;
pub mod vendors;

// use pdu::{Pdu, PduParseError};
use core::array::TryFromSliceError;
use core::str::Utf8Error;
use smoltcp::wire::{EthernetAddress, EthernetProtocol};

const LEN_MASK: u16 = 0b0000_0111_1111_1111;
// TODO: Un-pub
pub const ETHERCAT_ETHERTYPE: EthernetProtocol = EthernetProtocol::Unknown(0x88a4);
pub const MASTER_ADDR: EthernetAddress = EthernetAddress([0x10, 0x10, 0x10, 0x10, 0x10, 0x10]);

const BASE_SLAVE_ADDR: u16 = 0x1000;

#[cfg(not(target_endian = "little"))]
compile_error!("Only little-endian targets are supported at this time as primitive integers are cast to slices as-is");

pub trait PduData: Sized {
    const LEN: u16;

    type Error;

    fn len() -> u16 {
        Self::LEN & LEN_MASK
    }

    fn try_from_slice(slice: &[u8]) -> Result<Self, Self::Error>;
    fn as_slice(&self) -> &[u8];
}

macro_rules! impl_pdudata {
    ($ty:ty) => {
        impl PduData for $ty {
            const LEN: u16 = Self::BITS as u16 / 8;
            type Error = TryFromSliceError;

            fn try_from_slice(slice: &[u8]) -> Result<Self, Self::Error> {
                Ok(Self::from_le_bytes(slice.try_into()?))
            }

            fn as_slice<'a>(&'a self) -> &'a [u8] {
                // SAFETY: Copied from `safe-transmute` crate so I'm assuming...
                // SAFETY: EtherCAT is little-endian on the wire, so this will ONLY work on
                // little-endian targets, hence the `compile_error!()` above.
                unsafe {
                    core::slice::from_raw_parts(
                        self as *const Self as *const u8,
                        core::mem::size_of::<Self>(),
                    )
                }
            }
        }
    };
}

impl_pdudata!(u8);
impl_pdudata!(u16);
impl_pdudata!(u32);
impl_pdudata!(u64);
impl_pdudata!(i8);
impl_pdudata!(i16);
impl_pdudata!(i32);
impl_pdudata!(i64);

impl<const N: usize> PduData for [u8; N] {
    const LEN: u16 = N as u16;

    type Error = TryFromSliceError;

    fn try_from_slice(slice: &[u8]) -> Result<Self, Self::Error> {
        slice.try_into()
    }

    fn as_slice(&self) -> &[u8] {
        self
    }
}

/// A "Visible String" representation. Characters are specified to be within the ASCII range.
// TODO: Implement for `std::String` with a feature switch
impl<const N: usize> PduData for heapless::String<N> {
    const LEN: u16 = N as u16;

    type Error = VisibleStringError;

    fn try_from_slice(slice: &[u8]) -> Result<Self, Self::Error> {
        let mut out = heapless::String::new();

        out.push_str(core::str::from_utf8(slice).map_err(|e| VisibleStringError::Decode(e))?)
            .map_err(|_| VisibleStringError::TooLong)?;

        Ok(out)
    }

    fn as_slice(&self) -> &[u8] {
        self.as_bytes()
    }
}

pub enum VisibleStringError {
    Decode(Utf8Error),
    TooLong,
}
