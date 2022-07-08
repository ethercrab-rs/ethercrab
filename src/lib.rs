#![cfg_attr(not(feature = "std"), no_std)]

pub mod client;
pub mod command;
pub mod frame;
pub mod pdu;
pub mod register;
pub mod timer_factory;

// use pdu::{Pdu, PduParseError};
use core::array::TryFromSliceError;
use core::str::Utf8Error;
use smoltcp::wire::{EthernetAddress, EthernetProtocol};

const LEN_MASK: u16 = 0b0000_0111_1111_1111;
// TODO: Un-pub
pub const ETHERCAT_ETHERTYPE: EthernetProtocol = EthernetProtocol::Unknown(0x88a4);
pub const MASTER_ADDR: EthernetAddress = EthernetAddress([0x10, 0x10, 0x10, 0x10, 0x10, 0x10]);

pub trait PduData: Sized {
    const LEN: u16;

    type Error;

    fn len() -> u16 {
        Self::LEN & LEN_MASK
    }

    fn try_from_slice(slice: &[u8]) -> Result<Self, Self::Error>;
}

macro_rules! impl_pdudata {
    ($ty:ty) => {
        impl PduData for $ty {
            const LEN: u16 = Self::BITS as u16 / 8;
            type Error = TryFromSliceError;

            fn try_from_slice(slice: &[u8]) -> Result<Self, Self::Error> {
                Ok(Self::from_le_bytes(slice.try_into()?))
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
}

pub enum VisibleStringError {
    Decode(Utf8Error),
    TooLong,
}
