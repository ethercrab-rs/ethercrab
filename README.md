# EtherCrab

[![Build Status](https://circleci.com/gh/ethercrab-rs/ethercrab/tree/main.svg?style=shield)](https://circleci.com/gh/ethercrab-rs/ethercrab/tree/main)
[![Crates.io](https://img.shields.io/crates/v/ethercrab.svg)](https://crates.io/crates/ethercrab)
[![Docs.rs](https://docs.rs/ethercrab/badge.svg)](https://docs.rs/ethercrab)
[![Matrix chat](https://img.shields.io/matrix/ethercrab:matrix.org)](https://matrix.to/#/#ethercrab:matrix.org)

A performant, `async`-first EtherCAT MainDevice written in pure Rust.

## Crate features

- `std` (enabled by default) - exposes the `std` module, containing helpers to run the TX/RX
  loop on desktop operating systems.
- `defmt` - enable logging with the [`defmt`](https://docs.rs/defmt) crate.
- `log` - enable logging with the [`log`](https://docs.rs/log) crate. This is enabled by default
  when the `std` feature is enabled.
- `serde` - enable `serde` impls for some public items.
- `xdp` - enable support for XDP on some (currently only Linux) systems.

For `no_std` targets, it is recommended to add this crate with

```bash
cargo add --no-default-features --features defmt
```

## Examples

This example increments the output bytes of all detected SubDevices every tick. It is tested on an
EK1100 with output modules but may work on other basic SubDevices.

Run with e.g.

Linux

```bash
RUST_LOG=debug cargo run --example ek1100 --release -- eth0
```

Windows

```ps
$env:RUST_LOG="debug" ; cargo run --example ek1100 --release -- '\Device\NPF_{FF0ACEE6-E8CD-48D5-A399-619CD2340465}'
```

```rust
use env_logger::Env;
use ethercrab::{
    error::Error, std::{ethercat_now, tx_rx_task}, MainDevice, MainDeviceConfig, PduStorage, Timeouts
};
use std::{sync::Arc, time::Duration};
use tokio::time::MissedTickBehavior;

/// Maximum number of SubDevices that can be stored. This must be a power of 2 greater than 1.
const MAX_SUBDEVICES: usize = 16;
/// Maximum PDU data payload size - set this to the max PDI size or higher.
const MAX_PDU_DATA: usize = 1100;
/// Maximum number of EtherCAT frames that can be in flight at any one time.
const MAX_FRAMES: usize = 16;
/// Maximum total PDI length.
const PDI_LEN: usize = 64;

static PDU_STORAGE: PduStorage<MAX_FRAMES, MAX_PDU_DATA> = PduStorage::new();

#[tokio::main]
async fn main() -> Result<(), Error> {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    let interface = std::env::args()
        .nth(1)
        .expect("Provide network interface as first argument.");

    log::info!("Starting EK1100 demo...");
    log::info!("Ensure an EK1100 is the first SubDevice, with any number of modules connected after");
    log::info!("Run with RUST_LOG=ethercrab=debug or =trace for debug information");

    let (tx, rx, pdu_loop) = PDU_STORAGE.try_split().expect("can only split once");

    let maindevice = Arc::new(MainDevice::new(
        pdu_loop,
        Timeouts {
            wait_loop_delay: Duration::from_millis(2),
            mailbox_response: Duration::from_millis(1000),
            ..Default::default()
        },
        MainDeviceConfig::default(),
    ));

    tokio::spawn(tx_rx_task(&interface, tx, rx).expect("spawn TX/RX task"));

    let mut group = maindevice
        .init_single_group::<MAX_SUBDEVICES, PDI_LEN>(ethercat_now)
        .await
        .expect("Init");

    log::info!("Discovered {} SubDevices", group.len());

    for subdevice in group.iter(&maindevice) {
        // Special case: if an EL3004 module is discovered, it needs some specific config during
        // init to function properly
        if subdevice.name() == "EL3004" {
            log::info!("Found EL3004. Configuring...");

            subdevice.sdo_write(0x1c12, 0, 0u8).await?;
            subdevice.sdo_write(0x1c13, 0, 0u8).await?;

            subdevice.sdo_write(0x1c13, 1, 0x1a00u16).await?;
            subdevice.sdo_write(0x1c13, 2, 0x1a02u16).await?;
            subdevice.sdo_write(0x1c13, 3, 0x1a04u16).await?;
            subdevice.sdo_write(0x1c13, 4, 0x1a06u16).await?;
            subdevice.sdo_write(0x1c13, 0, 4u8).await?;
        }
    }

    let mut group = group.into_op(&maindevice).await.expect("PRE-OP -> OP");

    for subdevice in group.iter(&maindevice) {
        let io = subdevice.io_raw();

        log::info!(
            "-> SubDevice {:#06x} {} inputs: {} bytes, outputs: {} bytes",
            subdevice.configured_address(),
            subdevice.name(),
            io.inputs().len(),
            io.outputs().len()
        );
    }

    let mut tick_interval = tokio::time::interval(Duration::from_millis(5));
    tick_interval.set_missed_tick_behavior(MissedTickBehavior::Skip);

    loop {
        group.tx_rx(&maindevice).await.expect("TX/RX");

        // Increment every output byte for every SubDevice by one
        for mut subdevice in group.iter(&maindevice) {
            let mut io = subdevice.io_raw_mut();

            for byte in io.outputs().iter_mut() {
                *byte = byte.wrapping_add(1);
            }
        }

        tick_interval.tick().await;
    }
}
```

## Community

[We're on Matrix!](https://matrix.to/#/#ethercrab:matrix.org)

## Current and future features

- [x] `async` API
- [x] Usable in `no_std` contexts with no allocator required, as long as an `async` executor is available.
  - [x] Tested with [Embassy](https://embassy.dev)
  - [ ] Tested with [RTIC](https://rtic.rs/2/book/en/)
- [x] Autoconfigure SubDevices from their EEPROM (SII) data during startup
  - [x] Supports configuration using CoE data
- [x] Safely usable in multi-threaded Linux systems with e.g. `smol`, `tokio` or `std::thread` and
      `block_on`.
- [x] Support for `io_uring` on Linux systems to improve performance and latency
- [x] Support for SDO read/writes to configure SubDevices
- [x] Distributed clocks
  - [x] Detection of delays between SubDevices in topology
  - [x] Static drift compensation on startup
  - [x] Cyclic synchronisation during OP
- [x] Basic support for [CiA402](https://www.can-cia.org/can-knowledge/canopen/cia402/)/DS402 drives
  - [ ] A higher level DS402 API for torque, position and velocity control of common servo drives in
        a more abstract way.
- [ ] Integration with LinuxCNC as a HAL component using
      [the `linuxcnc-hal` crate](https://github.com/jamwaffles/linuxcnc-hal-rs).
- [ ] Load SubDevice configurations from ESI XML files

## Sponsors

![GitHub Sponsors](https://img.shields.io/github/sponsors/jamwaffles)

Thank you to everyone who has donated test equipment, time or money to the EtherCrab project! Would
you like to be in this list? Then please consider
[becoming a Github sponsor](https://github.com/sponsors/jamwaffles)!

- [@nealsjoe](https://twitter.com/nealsjoe) generously donated an EK1100 with several IO modules for
  testing with.
- [Trisk Bio](https://triskbio.com/) generously donated some additional Beckhoff modules and some
  optical ethernet gear.
- Smark sent a $200 one time donation. Thank you!

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or
  http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.
