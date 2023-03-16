# EtherCrab

[![Build Status](https://circleci.com/gh/ethercrab-rs/ethercrab/tree/master.svg?style=shield)](https://circleci.com/gh/ethercrab-rs/ethercrab/tree/master)
[![Crates.io](https://img.shields.io/crates/v/ethercrab.svg)](https://crates.io/crates/ethercrab)
[![Docs.rs](https://docs.rs/ethercrab/badge.svg)](https://docs.rs/ethercrab)
[![Matrix chat](https://img.shields.io/matrix/ethercrab:matrix.org)](https://matrix.to/#/#ethercrab:matrix.org)

An EtherCAT master written in pure Rust.

## Community

[We're on Matrix!](https://matrix.to/#/#ethercrab:matrix.org)

## MSRV

The current MSRV for EtherCrab is 1.68.

## Example

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
use async_ctrlc::CtrlC;
use async_io::Timer;
use ethercrab::{
    error::Error, std::tx_rx_task, Client, PduLoop, PduStorage, SlaveGroup, SubIndex, Timeouts,
};
use futures_lite::{FutureExt, StreamExt};
use smol::LocalExecutor;
use std::{sync::Arc, time::Duration};

/// Maximum number of slaves that can be stored.
const MAX_SLAVES: usize = 16;
/// Maximum PDU data payload size - set this to the max PDI size or higher.
const MAX_PDU_DATA: usize = 1100;
/// Maximum number of EtherCAT frames that can be in flight at any one time.
const MAX_FRAMES: usize = 16;
/// Maximum total PDI length.
const PDI_LEN: usize = 64;

static PDU_STORAGE: PduStorage<MAX_FRAMES, MAX_PDU_DATA> = PduStorage::new();

async fn main_inner(ex: &LocalExecutor<'static>) -> Result<(), Error> {
    let interface = std::env::args()
        .nth(1)
        .expect("Provide interface as first argument");

    log::info!("Starting EtherCrab demo...");
    log::info!("Ensure an EK1100 is the first slave, with any number of modules connected after");

    let (tx, rx, pdu_loop) = PDU_STORAGE.try_split().expect("can only split once");

    let client = Arc::new(Client::new(pdu_loop, Timeouts::default()));

    ex.spawn(tx_rx_task(&interface, tx, rx).unwrap()).detach();

    let group = SlaveGroup::<MAX_SLAVES, PDI_LEN, _>::new(|slave| {
        Box::pin(async {
            // EL3004 needs some specific config during init to function properly
            if slave.name() == "EL3004" {
                log::info!("Found EL3004. Configuring...");

                slave.write_sdo(0x1c12, SubIndex::Index(0), 0u8).await?;
                slave.write_sdo(0x1c13, SubIndex::Index(0), 0u8).await?;

                slave
                    .write_sdo(0x1c13, SubIndex::Index(1), 0x1a00u16)
                    .await?;
                slave
                    .write_sdo(0x1c13, SubIndex::Index(2), 0x1a02u16)
                    .await?;
                slave
                    .write_sdo(0x1c13, SubIndex::Index(3), 0x1a04u16)
                    .await?;
                slave
                    .write_sdo(0x1c13, SubIndex::Index(4), 0x1a06u16)
                    .await?;
                slave.write_sdo(0x1c13, SubIndex::Index(0), 4u8).await?;
            }

            Ok(())
        })
    });

    let group = client
        // Initialise up to 16 slave devices
        .init::<MAX_SLAVES, _>(group, |groups, slave| groups.push(slave))
        .await
        .expect("Init");

    log::info!("Group has {} slaves", group.len());

    for slave in group.slaves() {
        let (i, o) = slave.io();

        log::info!(
            "-> Slave {} {} has {} input bytes, {} output bytes",
            slave.configured_address,
            slave.name,
            i.len(),
            o.len(),
        );
    }

    let mut tick_interval = Timer::interval(Duration::from_millis(5));

    let group = Arc::new(group);
    let group2 = group.clone();

    while let Some(_) = tick_interval.next().await {
        group.tx_rx(&client).await.expect("TX/RX");

        // Increment every output byte for every slave device by one
        for slave in group2.slaves() {
            let (_i, o) = slave.io();

            for byte in o.iter_mut() {
                *byte = byte.wrapping_add(1);
            }
        }
    }

    Ok(())
}

fn main() -> Result<(), Error> {
    env_logger::init();
    let local_ex = LocalExecutor::new();

    let ctrlc = CtrlC::new().expect("cannot create Ctrl+C handler?");

    futures_lite::future::block_on(
        local_ex.run(ctrlc.race(async { main_inner(&local_ex).await.unwrap() })),
    );

    Ok(())
}
```

## Current goals

- [x] Become a member of the
      [EtherCAT Technologies Group (ETG)](https://www.ethercat.org/default.htm) and get access to
      the EtherCAT specification.
- [x] Explore basic master architecture to support current design goals
- [x] Autoconfigure slaves from their EEPROM data
  - [x] Also support configuration using CoE data
- [x] A first pass at a safe `async` API
  - [ ] Tested in no_std environments with either [RTIC](https://rtic.rs) (once async support is
        released) or [Embassy](https://embassy.dev/)
- [x] Usable in multi-threaded Linux systems with e.g. `tokio` or `std::thread` and `block_on`.
- [x] Configuration and cyclic communication with multiple EtherCAT slaves.
- [ ] Basic support for [CiA402](https://www.can-cia.org/can-knowledge/canopen/cia402/) torque,
      position and velocity control of common servo drives in a high-level way.

Current test hardware is an EK1100 + modules and two LAN9252 dev boards.

## Future goals

These may change at any time.

- [-] ~~A blocking API which spins on internal futures for best compatibility, possibly using
  [casette](https://lib.rs/crates/cassette) or [nb-executor](https://lib.rs/crates/nb-executor).~~
- [ ] Integration with LinuxCNC as a HAL component using
      [the Rust `linuxcnc-hal`](https://github.com/jamwaffles/linuxcnc-hal-rs).
- [ ] A multiplatform configuration/debugging/management GUI
- [ ] Loading slave configurations from ESI XML files

  Current test hardware consists of a Kollmorgen AKD servo drive and three Leadshine EL7 servo
  drives

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
