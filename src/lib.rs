//! A performant, `async`-first EtherCAT MainDevice written in pure Rust.
//!
//! # Crate features
//!
//! - `std` (enabled by default) - exposes the [`std`] module, containing helpers to run the TX/RX
//!   loop on desktop operating systems.
//! - `defmt` - enable logging with the [`defmt`](https://docs.rs/defmt) crate.
//! - `log` - enable logging with the [`log`](https://docs.rs/log) crate. This is enabled by default
//!   when the `std` feature is enabled.
//! - `serde` - enable `serde` impls for some public items.
//!
//! For `no_std` targets, it is recommended to add this crate with
//!
//! ```bash
//! cargo add --no-default-features --features defmt
//! ```
//!
//! # Examples
//!
//! This example increments the output bytes of all detected SubDevices every tick. It is tested on an
//! EK1100 with output modules but may work on other basic SubDevices.
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
//!     error::Error, std::{ethercat_now, tx_rx_task}, MainDevice, MainDeviceConfig, PduStorage, Timeouts
//! };
//! use std::{sync::Arc, time::Duration};
//! use tokio::time::MissedTickBehavior;
//!
//! /// Maximum number of SubDevices that can be stored. This must be a power of 2 greater than 1.
//! const MAX_SUBDEVICES: usize = 16;
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
//!     log::info!("Ensure an EK1100 is the first SubDevice, with any number of modules connected after");
//!     log::info!("Run with RUST_LOG=ethercrab=debug or =trace for debug information");
//!
//!     let (tx, rx, pdu_loop) = PDU_STORAGE.try_split().expect("can only split once");
//!
//!     let maindevice = Arc::new(MainDevice::new(
//!         pdu_loop,
//!         Timeouts {
//!             wait_loop_delay: Duration::from_millis(2),
//!             mailbox_response: Duration::from_millis(1000),
//!             ..Default::default()
//!         },
//!         MainDeviceConfig::default(),
//!     ));
//!
//!     tokio::spawn(tx_rx_task(&interface, tx, rx).expect("spawn TX/RX task"));
//!
//!     let mut group = maindevice
//!         .init_single_group::<MAX_SUBDEVICES, PDI_LEN>(ethercat_now)
//!         .await
//!         .expect("Init");
//!
//!     log::info!("Discovered {} SubDevices", group.len());
//!
//!     for subdevice in group.iter(&maindevice) {
//!         // Special case: if an EL3004 module is discovered, it needs some specific config during
//!         // init to function properly
//!         if subdevice.name() == "EL3004" {
//!             log::info!("Found EL3004. Configuring...");
//!
//!             subdevice.sdo_write(0x1c12, 0, 0u8).await?;
//!             subdevice.sdo_write(0x1c13, 0, 0u8).await?;
//!
//!             subdevice.sdo_write(0x1c13, 1, 0x1a00u16).await?;
//!             subdevice.sdo_write(0x1c13, 2, 0x1a02u16).await?;
//!             subdevice.sdo_write(0x1c13, 3, 0x1a04u16).await?;
//!             subdevice.sdo_write(0x1c13, 4, 0x1a06u16).await?;
//!             subdevice.sdo_write(0x1c13, 0, 4u8).await?;
//!         }
//!     }
//!
//!     let mut group = group.into_op(&maindevice).await.expect("PRE-OP -> OP");
//!
//!     for subdevice in group.iter(&maindevice) {
//!         let io = subdevice.io_raw();
//!
//!         log::info!(
//!             "-> SubDevice {:#06x} {} inputs: {} bytes, outputs: {} bytes",
//!             subdevice.configured_address(),
//!             subdevice.name(),
//!             io.inputs().len(),
//!             io.outputs().len()
//!         );
//!     }
//!
//!     let mut tick_interval = tokio::time::interval(Duration::from_millis(5));
//!     tick_interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
//!
//!     loop {
//!         group.tx_rx(&maindevice).await.expect("TX/RX");
//!
//!         // Increment every output byte for every SubDevice by one
//!         for mut subdevice in group.iter(&maindevice) {
//!             let mut io = subdevice.io_raw_mut();
//!
//!             for byte in io.outputs().iter_mut() {
//!                 *byte = byte.wrapping_add(1);
//!             }
//!         }
//!
//!         tick_interval.tick().await;
//!     }
//! }
//! ```

#![cfg_attr(not(feature = "std"), no_std)]
#![deny(missing_docs)]
#![deny(missing_copy_implementations)]
#![deny(trivial_casts)]
#![deny(trivial_numeric_casts)]
#![deny(unused_import_braces)]
#![deny(rustdoc::broken_intra_doc_links)]
#![deny(rustdoc::private_intra_doc_links)]
#![doc(
    html_logo_url = "https://raw.githubusercontent.com/ethercrab-rs/ethercrab/5185a914f49abad8b0f71a1b3b689500077e7c2b/ethercrab-logo-docsrs.svg"
)]
#![doc(
    html_favicon_url = "https://raw.githubusercontent.com/ethercrab-rs/ethercrab/5185a914f49abad8b0f71a1b3b689500077e7c2b/ethercrab-logo-docsrs.svg"
)]

// MUST go first so everything else can see the macros inside
pub(crate) mod fmt;

mod al_control;
mod al_status_code;
mod base_data_types;
mod coe;
mod command;
mod dc;
mod dl_status;
mod eeprom;
pub mod error;
mod ethernet;
mod fmmu;
mod generate;
mod mailbox;
mod maindevice;
mod maindevice_config;
mod pdi;
mod pdu_loop;
mod register;
mod subdevice;
pub mod subdevice_group;
mod subdevice_state;
mod sync_manager_channel;
mod timer_factory;
mod vendors;

#[cfg(feature = "__internals")]
pub mod internals;

#[cfg(feature = "std")]
pub mod std;

pub use al_status_code::AlStatusCode;
pub use coe::SubIndex;
pub use command::{Command, Reads, WrappedRead, WrappedWrite, Writes};
pub use ethercrab_wire::{
    EtherCrabWireRead, EtherCrabWireReadSized, EtherCrabWireReadWrite, EtherCrabWireSized,
    EtherCrabWireWrite, EtherCrabWireWriteSized,
};
use ethernet::EthernetAddress;
pub use maindevice::MainDevice;
pub use maindevice_config::{MainDeviceConfig, RetryBehaviour};
pub use pdu_loop::{PduLoop, PduRx, PduStorage, PduTx, ReceiveAction, SendableFrame};
pub use register::{DcSupport, RegisterAddress};
pub use subdevice::{DcSync, SubDevice, SubDeviceIdentity, SubDevicePdi, SubDeviceRef};
pub use subdevice_group::{GroupId, SubDeviceGroup, SubDeviceGroupHandle, TxRxResponse};
pub use subdevice_state::SubDeviceState;
pub use timer_factory::Timeouts;

const LEN_MASK: u16 = 0b0000_0111_1111_1111;
const ETHERCAT_ETHERTYPE: u16 = 0x88a4;
const MASTER_ADDR: EthernetAddress = EthernetAddress([0x10, 0x10, 0x10, 0x10, 0x10, 0x10]);

/// Starting address for discovered subdevices.
const BASE_SUBDEVICE_ADDRESS: u16 = 0x1000;

#[cfg(feature = "std")]
type SpinStrategy = spin::Yield;
#[cfg(not(feature = "std"))]
type SpinStrategy = spin::Spin;

#[allow(unused)]
fn test_logger() {
    #[cfg(all(not(miri), test))]
    let _ = env_logger::builder().is_test(true).try_init();

    #[cfg(all(miri, test))]
    let _ = simple_logger::init_with_level(log::Level::Debug);
}
