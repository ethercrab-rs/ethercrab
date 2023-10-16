# EtherCrab

[![Build Status](https://circleci.com/gh/ethercrab-rs/ethercrab/tree/master.svg?style=shield)](https://circleci.com/gh/ethercrab-rs/ethercrab/tree/master)
[![Crates.io](https://img.shields.io/crates/v/ethercrab.svg)](https://crates.io/crates/ethercrab)
[![Docs.rs](https://docs.rs/ethercrab/badge.svg)](https://docs.rs/ethercrab)
[![Matrix chat](https://img.shields.io/matrix/ethercrab:matrix.org)](https://matrix.to/#/#ethercrab:matrix.org)

A performant, `async`-first EtherCAT master written in pure Rust.

> <div style="padding: var(--fbc-font-size); background: var(--code-block-background-color)">
>
> Are you looking to use Rust in your next EtherCAT deployment? Commercial support for EtherCrab
> is available! Send me a message at [james@wapl.es](mailto:james@wapl.es) to get started with a
> free consulting call.
>
> </div>

## Crate features

- `std` (enabled by default) - exposes the [`std`] module, containing helpers to run the TX/RX
  loop on desktop operating systems.
- `defmt` - enable logging with the [`defmt`](https://docs.rs/defmt) crate.
- `log` - enable logging with the [`log`](https://docs.rs/log) crate. This is enabled by default
  when the `std` feature is enabled.
- `serde` - enable `serde` impls for some public items.

For `no_std` targets, it is recommended to add this crate with

```bash
cargo add --no-default-features --features defmt
```

## Examples

This example increments the output bytes of all detected slaves every tick. It is tested on an
EK1100 with output modules but may work on other basic slave devices.

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
    error::Error, std::tx_rx_task, Client, ClientConfig, PduStorage, SlaveGroup, Timeouts,
    slave_group
};
use std::{sync::Arc, time::Duration};
use tokio::time::MissedTickBehavior;

/// Maximum number of slaves that can be stored. This must be a power of 2 greater than 1.
const MAX_SLAVES: usize = 16;
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
    log::info!("Ensure an EK1100 is the first slave, with any number of modules connected after");
    log::info!("Run with RUST_LOG=ethercrab=debug or =trace for debug information");

    let (tx, rx, pdu_loop) = PDU_STORAGE.try_split().expect("can only split once");

    let client = Arc::new(Client::new(
        pdu_loop,
        Timeouts {
            wait_loop_delay: Duration::from_millis(2),
            mailbox_response: Duration::from_millis(1000),
            ..Default::default()
        },
        ClientConfig::default(),
    ));

    tokio::spawn(tx_rx_task(&interface, tx, rx).expect("spawn TX/RX task"));

    let mut group = client
        .init_single_group::<MAX_SLAVES, PDI_LEN>()
        .await
        .expect("Init");

    log::info!("Discovered {} slaves", group.len());

    for slave in group.iter(&client) {
        // Special case: if an EL3004 module is discovered, it needs some specific config during
        // init to function properly
        if slave.name() == "EL3004" {
            log::info!("Found EL3004. Configuring...");

            slave.sdo_write(0x1c12, 0, 0u8).await?;
            slave.sdo_write(0x1c13, 0, 0u8).await?;

            slave.sdo_write(0x1c13, 1, 0x1a00u16).await?;
            slave.sdo_write(0x1c13, 2, 0x1a02u16).await?;
            slave.sdo_write(0x1c13, 3, 0x1a04u16).await?;
            slave.sdo_write(0x1c13, 4, 0x1a06u16).await?;
            slave.sdo_write(0x1c13, 0, 4u8).await?;
        }
    }

    let mut group = group.into_op(&client).await.expect("PRE-OP -> OP");

    for slave in group.iter(&client) {
        let (i, o) = slave.io_raw();

        log::info!(
            "-> Slave {:#06x} {} inputs: {} bytes, outputs: {} bytes",
            slave.configured_address(),
            slave.name(),
            i.len(),
            o.len()
        );
    }

    let mut tick_interval = tokio::time::interval(Duration::from_millis(5));
    tick_interval.set_missed_tick_behavior(MissedTickBehavior::Skip);

    loop {
        group.tx_rx(&client).await.expect("TX/RX");

        // Increment every output byte for every slave device by one
        for slave in group.iter(&client) {
            let (_i, o) = slave.io_raw();

            for byte in o.iter_mut() {
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
- [x] Autoconfigure slaves from their EEPROM (SII) data during startup
  - [x] Supports configuration using CoE data
- [x] Safely usable in multi-threaded Linux systems with e.g. `smol`, `tokio` or `std::thread` and
      `block_on`.
- [x] Support for SDO read/writes to configure slave devices
- [ ] Distributed clocks
  - [x] Detection of delays between slave devices in topology
  - [x] Static drift compensation on startup
  - [ ] Cyclic synchronisation during OP
- [x] Basic support for [CiA402](https://www.can-cia.org/can-knowledge/canopen/cia402/)/DS402 drives
  - [ ] A higher level DS402 API for torque, position and velocity control of common servo drives in
        a more abstract way.
- [ ] Integration with LinuxCNC as a HAL component using
      [the `linuxcnc-hal` crate](https://github.com/jamwaffles/linuxcnc-hal-rs).
- [ ] Load slave configurations from ESI XML files

## Sponsors

![GitHub Sponsors](https://img.shields.io/github/sponsors/jamwaffles)

Thank you to everyone who has donated test equipment, time or money to the EtherCrab project! Would
you like to be in this list? Then please consider
[becoming a Github sponsor](https://github.com/sponsors/jamwaffles)!

- [@nealsjoe](https://twitter.com/nealsjoe) generously donated an EK1100 with several IO modules for
  testing with.
- [Trisk Bio](https://triskbio.com/) generously donated some additional Beckhoff modules and some
  optical ethernet gear.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or
  http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.
