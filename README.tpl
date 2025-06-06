# EtherCrab

[![Build Status](https://circleci.com/gh/ethercrab-rs/ethercrab/tree/main.svg?style=shield)](https://circleci.com/gh/ethercrab-rs/ethercrab/tree/main)
[![Crates.io](https://img.shields.io/crates/v/ethercrab.svg)](https://crates.io/crates/ethercrab)
[![Docs.rs](https://docs.rs/ethercrab/badge.svg)](https://docs.rs/ethercrab)
[![Matrix chat](https://img.shields.io/matrix/ethercrab:matrix.org)](https://matrix.to/#/#ethercrab:matrix.org)

{{readme}}

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
