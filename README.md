# EtherCrab

[![Build Status](https://circleci.com/gh/ethercrab-rs/ethercrab/tree/master.svg?style=shield)](https://circleci.com/gh/ethercrab-rs/ethercrab/tree/master)
[![Crates.io](https://img.shields.io/crates/v/ethercrab.svg)](https://crates.io/crates/ethercrab)
[![Docs.rs](https://docs.rs/ethercrab/badge.svg)](https://docs.rs/ethercrab)
[![Matrix chat](https://img.shields.io/matrix/ethercrab:matrix.org)](https://matrix.to/#/#ethercrab:matrix.org)

An EtherCAT master written in pure Rust.

EtherCrab is looking for sponsors! I (@jamwaffles) am developing EtherCrab in my spare time with
currently no fulltime income, so if you want a safe, performant Rust EtherCat master please consider
becoming a sponsor!

## Community

[We're on Matrix!](https://matrix.to/#/#ethercrab:matrix.org)

## MSRV

Unfortunately, nightly Rust is currently required.

The MSRV for EtherCrab can be found in `rust-toolchain.toml`.

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

Thank you to everyone who has donated test equipment, time or money to the EtherCrab project! To
help the EtherCrab project progress faster, please consider becoming a sponsor, or donating EtherCAT
hardware to ensure best compatibility.

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
