//! An EtherCAT master written in pure Rust.
//!
//! This crate is in its very early stages, however it has been used to control Beckhoff EK1100
//! modules, Kollmorgen and LeadShine EC400 servo drives under Windows and Linux. Examples and
//! documentation is sparse, but will be improved in future releases.
//!
//! Please note that this crate currently requires nightly Rust.
//!
//! Breaking changes may be made at any time.
//!
//! # MSRV
//!
//! The current MSRV for EtherCrab is 1.68.
//!
//! # Examples
//!
//! This example increments the output bytes of all detected slaves every tick. It is tested on an
//! EK1100 with output modules but may work on other basic slave devices.
//!
//! Run with e.g.
//!
//! Linux
//!
//! ```bash
//! RUST_LOG=debug cargo run --example ek1100 --release -- eth0
//! ```
//!
//! Windows
//!
//! ```ps
//! $env:RUST_LOG="debug" ; cargo run --example ek1100 --release -- '\Device\NPF_{FF0ACEE6-E8CD-48D5-A399-619CD2340465}'
//! ```
//!
//! ```rust,no_run
//! use env_logger::Env;
//! use ethercrab::{
//!     error::Error, std::tx_rx_task, Client, ClientConfig, PduStorage, SlaveGroup, Timeouts,
//! };
//! use std::{sync::Arc, time::Duration};
//! use tokio::time::MissedTickBehavior;
//!
//! /// Maximum number of slaves that can be stored. This must be a power of 2 greater than 1.
//! const MAX_SLAVES: usize = 16;
//! /// Maximum PDU data payload size - set this to the max PDI size or higher.
//! const MAX_PDU_DATA: usize = 1100;
//! /// Maximum number of EtherCAT frames that can be in flight at any one time.
//! const MAX_FRAMES: usize = 16;
//! /// Maximum total PDI length.
//! const PDI_LEN: usize = 64;
//!
//! static PDU_STORAGE: PduStorage<MAX_FRAMES, MAX_PDU_DATA> = PduStorage::new();
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Error> {
//!     env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();
//!
//!     let interface = std::env::args()
//!         .nth(1)
//!         .expect("Provide network interface as first argument.");
//!
//!     log::info!("Starting EK1100 demo...");
//!     log::info!("Ensure an EK1100 is the first slave, with any number of modules connected after");
//!     log::info!("Run with RUST_LOG=ethercrab=debug or =trace for debug information");
//!
//!     let (tx, rx, pdu_loop) = PDU_STORAGE.try_split().expect("can only split once");
//!
//!     let client = Arc::new(Client::new(
//!         pdu_loop,
//!         Timeouts {
//!             wait_loop_delay: Duration::from_millis(2),
//!             mailbox_response: Duration::from_millis(1000),
//!             ..Default::default()
//!         },
//!         ClientConfig::default(),
//!     ));
//!
//!     tokio::spawn(tx_rx_task(&interface, tx, rx).expect("spawn TX/RX task"));
//!
//!     let group = SlaveGroup::<MAX_SLAVES, PDI_LEN>::new(|slave| {
//!         Box::pin(async {
//!             // Special case: if an EL3004 module is discovered, it needs some specific config during
//!             // init to function properly
//!             if slave.name() == "EL3004" {
//!                 log::info!("Found EL3004. Configuring...");
//!
//!                 slave.sdo_write(0x1c12, 0, 0u8).await?;
//!                 slave.sdo_write(0x1c13, 0, 0u8).await?;
//!
//!                 slave
//!                     .sdo_write(0x1c13, 1, 0x1a00u16)
//!                     .await?;
//!                 slave
//!                     .sdo_write(0x1c13, 2, 0x1a02u16)
//!                     .await?;
//!                 slave
//!                     .sdo_write(0x1c13, 3, 0x1a04u16)
//!                     .await?;
//!                 slave
//!                     .sdo_write(0x1c13, 4, 0x1a06u16)
//!                     .await?;
//!                 slave.sdo_write(0x1c13, 0, 4u8).await?;
//!             }
//!
//!             Ok(())
//!         })
//!     });
//!
//!     let group = client
//!         // Initialise a single group
//!         .init::<MAX_SLAVES, _>(group, |group, _slave| Ok(group))
//!         .await
//!         .expect("Init");
//!
//!     log::info!("Discovered {} slaves", group.len());
//!
//!     for slave in group.iter(&client) {
//!         let (i, o) = slave.io_raw();
//!
//!         log::info!(
//!             "-> Slave {} {} has {} input bytes, {} output bytes",
//!             slave.configured_address(),
//!             slave.name(),
//!             i.len(),
//!             o.len(),
//!         );
//!     }
//!
//!     let mut tick_interval = tokio::time::interval(Duration::from_millis(5));
//!     tick_interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
//!
//!     loop {
//!         group.tx_rx(&client).await.expect("TX/RX");
//!
//!         // Increment every output byte for every slave device by one
//!         for slave in group.iter(&client) {
//!             let (_i, o) = slave.io_raw();
//!
//!             for byte in o.iter_mut() {
//!                 *byte = byte.wrapping_add(1);
//!             }
//!         }
//!
//!         tick_interval.tick().await;
//!     }
//! }
//! ```

#![cfg_attr(not(feature = "std"), no_std)]
// #![deny(missing_docs)]
// #![deny(missing_copy_implementations)]
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
mod client_config;
mod coe;
mod command;
mod dc;
mod dl_status;
pub mod ds402;
mod eeprom;
pub mod error;
mod fmmu;
mod generate;
mod mailbox;
mod pdi;
mod pdu_data;
mod pdu_loop;
mod register;
pub mod slave;
mod slave_group;
mod slave_state;
mod sync_manager_channel;
mod timer_factory;
mod vendors;
pub mod convenience;

#[doc(hidden)]
pub mod internals;

#[cfg(feature = "std")]
pub mod std;

use nom::IResult;
use smoltcp::wire::{EthernetAddress, EthernetProtocol};

pub use al_status_code::AlStatusCode;
pub use client::Client;
pub use client_config::ClientConfig;
pub use coe::SubIndex;
pub use pdu_loop::{PduLoop, PduRx, PduStorage, PduTx};
pub use register::RegisterAddress;
pub use slave::{Slave, SlavePdi, SlaveRef};
pub use slave_group::{GroupId, GroupSlaveIterator, SlaveGroup, SlaveGroupHandle};
pub use slave_state::SlaveState;
pub use timer_factory::Timeouts;
pub use pdu_data::{PduData, PduStruct};
pub use convenience::{
	field::{Field, DType},
	sdo,
	};

const LEN_MASK: u16 = 0b0000_0111_1111_1111;
const ETHERCAT_ETHERTYPE_RAW: u16 = 0x88a4;
const ETHERCAT_ETHERTYPE: EthernetProtocol = EthernetProtocol::Unknown(ETHERCAT_ETHERTYPE_RAW);
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
