//! An EtherCAT master written in pure Rust.
//!
//! This crate is in its very early stages, however it has been used to control Beckhoff EK1100
//! modules, Kollmorgen and LeadShine EC400 servo drives under Windows and Linux. Examples and
//! documentation is sparse, but will be improved in future releases.
//!
//! Breaking changes may be made at any time.

#![feature(const_maybe_uninit_zeroed)]
#![feature(pin_macro)]
#![cfg_attr(not(feature = "std"), no_std)]
#![deny(missing_docs)]
#![deny(missing_copy_implementations)]
#![deny(trivial_casts)]
#![deny(trivial_numeric_casts)]
#![deny(unused_import_braces)]
#![deny(unused_qualifications)]
#![deny(rustdoc::broken_intra_doc_links)]
#![deny(rustdoc::private_intra_doc_links)]

// This mod MUST go first, so that the others see its macros.
pub(crate) mod log;

mod al_control;
mod al_status_code;
mod base_data_types;
mod client;
mod coe;
mod command;
mod dc;
mod dl_status;
pub mod ds402;
mod eeprom;
pub mod error;
mod fmmu;
mod mailbox;
mod pdi;
mod pdu_data;
mod pdu_loop;
mod register;
mod slave;
mod slave_group;
mod slave_state;
mod sync_manager_channel;
mod timer_factory;
mod vendors;

#[cfg(feature = "std")]
pub mod std;

use nom::IResult;
use smoltcp::wire::{EthernetAddress, EthernetProtocol};

pub use client::Client;
pub use coe::SubIndex;
pub use pdu_loop::{PduLoop, PduStorage};
pub use register::RegisterAddress;
pub use slave_group::SlaveGroup;
pub use slave_state::SlaveState;
pub use timer_factory::{Timeouts, TimerFactory};

// TODO: Deleteme
pub use slave_group::GroupSlave;

const LEN_MASK: u16 = 0b0000_0111_1111_1111;
const ETHERCAT_ETHERTYPE: EthernetProtocol = EthernetProtocol::Unknown(0x88a4);
const MASTER_ADDR: EthernetAddress = EthernetAddress([0x10, 0x10, 0x10, 0x10, 0x10, 0x10]);

/// Starting address for discovered slaves.
const BASE_SLAVE_ADDR: u16 = 0x1000;

/// Ensure that a buffer passed to a parsing function is fully consumed.
///
/// This mostly checks internal logic to ensure we don't miss data when parsing a struct.
fn all_consumed<'a, E>(i: &'a [u8]) -> IResult<&'a [u8], (), E>
where
    E: nom::error::ParseError<&'a [u8]>,
{
    if i.is_empty() {
        Ok((i, ()))
    } else {
        Err(nom::Err::Error(E::from_error_kind(
            i,
            nom::error::ErrorKind::Eof,
        )))
    }
}
