# EtherCrab

[![Build Status](https://circleci.com/gh/ethercrab-rs/ethercrab/tree/master.svg?style=shield)](https://circleci.com/gh/ethercrab-rs/ethercrab/tree/master)
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
- [x] Autoconfigure slaves from their EEPROM (SII) data during startup
  - [x] Supports configuration using CoE data
- [x] Safely usable in multi-threaded Linux systems with e.g. `tokio` or `std::thread` and
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
      [the Rust `linuxcnc-hal`](https://github.com/jamwaffles/linuxcnc-hal-rs).
- [ ] Load slave configurations from ESI XML files

### Profiling

To profile an example:

```bash
cargo build --example <example name> --profile profiling

# Might need sudo sysctl kernel.perf_event_paranoid=-1
# Might need sudo sysctl kernel.perf_event_mlock_kb=2048
sudo setcap cap_net_raw=pe ./target/profiling/examples/<example name>
sudo perf record ./target/profiling/examples/<example name> <example args>

# Ctrl + C when you're done

sudo chown $USER perf.data
samply load perf.data
```

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
