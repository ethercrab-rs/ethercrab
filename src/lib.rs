#![cfg_attr(not(feature = "std"), no_std)]

// This mod MUST go first, so that the others see its macros.
pub(crate) mod log;

// Taken from `futures-lite`
// TODO: Use core::pin::pin! when stablised
macro_rules! pin {
    ($($x:ident),* $(,)?) => {
        $(
            let mut $x = $x;
            #[allow(unused_mut)]
            let mut $x = unsafe {
                core::pin::Pin::new_unchecked(&mut $x)
            };
        )*
    }
}

mod al_control;
mod al_status_code;
mod base_data_types;
mod client;
mod command;
mod eeprom;
pub mod error;
mod fmmu;
mod mailbox;
mod pdi;
mod pdu_loop;
mod register;
// TODO: Un-pub
pub mod slave;
mod slave_group;
mod slave_state;
mod sync_manager_channel;
mod timer_factory;
mod vendors;

#[cfg(feature = "std")]
pub mod std;

use core::str::Utf8Error;
use core::{array::TryFromSliceError, time::Duration};
use embassy_futures::select::{select, Either};
use error::Error;
use smoltcp::wire::{EthernetAddress, EthernetProtocol};
use timer_factory::TimerFactory;

// TODO: Remove, or make a "low_level" module to allow inner access to services
pub use pdu_loop::CheckWorkingCounter;

pub use client::Client;
pub use pdi::Pdi;
pub use slave_state::SlaveState;

const LEN_MASK: u16 = 0b0000_0111_1111_1111;
const ETHERCAT_ETHERTYPE: EthernetProtocol = EthernetProtocol::Unknown(0x88a4);
const MASTER_ADDR: EthernetAddress = EthernetAddress([0x10, 0x10, 0x10, 0x10, 0x10, 0x10]);

const BASE_SLAVE_ADDR: u16 = 0x1000;

#[cfg(not(target_endian = "little"))]
compile_error!("Only little-endian targets are supported at this time as primitive integers are cast to slices as-is");

pub trait PduRead: Sized {
    const LEN: u16;

    type Error;

    fn len() -> u16 {
        Self::LEN & LEN_MASK
    }

    fn try_from_slice(slice: &[u8]) -> Result<Self, Self::Error>;
}

pub trait PduData: PduRead {
    fn as_slice(&self) -> &[u8];
}

macro_rules! impl_pdudata {
    ($ty:ty) => {
        impl PduRead for $ty {
            const LEN: u16 = Self::BITS as u16 / 8;
            type Error = TryFromSliceError;

            fn try_from_slice(slice: &[u8]) -> Result<Self, Self::Error> {
                Ok(Self::from_le_bytes(slice.try_into()?))
            }
        }

        impl PduData for $ty {
            fn as_slice<'a>(&'a self) -> &'a [u8] {
                // SAFETY: Copied from `safe-transmute` crate so I'm assuming...
                // SAFETY: EtherCAT is little-endian on the wire, so this will ONLY work on
                // little-endian targets, hence the `compile_error!()` above.
                // Clippy: "error: found a count of bytes instead of a count of elements of `T`"
                #[allow(clippy::size_of_in_element_count)]
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

impl<const N: usize> PduRead for [u8; N] {
    const LEN: u16 = N as u16;

    type Error = TryFromSliceError;

    fn try_from_slice(slice: &[u8]) -> Result<Self, Self::Error> {
        slice.try_into()
    }
}

impl<const N: usize> PduData for [u8; N] {
    fn as_slice(&self) -> &[u8] {
        self
    }
}

impl PduRead for () {
    const LEN: u16 = 0;

    type Error = TryFromSliceError;

    fn try_from_slice(_slice: &[u8]) -> Result<Self, Self::Error> {
        Ok(())
    }
}

impl PduData for () {
    fn as_slice(&self) -> &[u8] {
        &[]
    }
}

impl<const N: usize> PduRead for heapless::String<N> {
    const LEN: u16 = N as u16;

    type Error = VisibleStringError;

    fn try_from_slice(slice: &[u8]) -> Result<Self, Self::Error> {
        let mut out = heapless::String::new();

        out.push_str(core::str::from_utf8(slice).map_err(VisibleStringError::Decode)?)
            .map_err(|_| VisibleStringError::TooLong)?;

        Ok(out)
    }
}

/// A "Visible String" representation. Characters are specified to be within the ASCII range.
// TODO: Implement for `std::String` with a feature switch
impl<const N: usize> PduData for heapless::String<N> {
    fn as_slice(&self) -> &[u8] {
        self.as_bytes()
    }
}

pub enum VisibleStringError {
    Decode(Utf8Error),
    TooLong,
}

pub(crate) async fn timeout<TIMEOUT, O, F>(timeout: Duration, future: F) -> Result<O, Error>
where
    TIMEOUT: TimerFactory,
    F: core::future::Future<Output = Result<O, Error>>,
{
    pin!(future);

    match select(future, TIMEOUT::timer(timeout)).await {
        Either::First(res) => res,
        Either::Second(_timeout) => Err(Error::Timeout),
    }
}
