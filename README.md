# EtherCrab

An EtherCAT master written in pure Rust.

## Current goals

- [x] Become a member of the
      [EtherCAT Technologies Group (ETG)](https://www.ethercat.org/default.htm) and get access to
      the EtherCAT specification.
- [ ] (in progress) Explore basic master architecture to support current design goals
- [ ] Usable in no_std environments with either [RTIC](https://rtic.rs) or
      [Embassy](https://embassy.dev/)
- [ ] Usable in multi-threaded Linux systems with optional realtime support via the PREEMPT-RT
      patches
- [ ] Configuration and cyclic communication with multiple EtherCAT slaves.

  Current test hardware is an EK1100 + modules and two LAN9252 dev boards.

- [ ] Support for [CiA402](https://www.can-cia.org/can-knowledge/canopen/cia402/) torque, position
      and velocity control of common servo drives in a high-level way.

  Current test hardware consists of a Kollmorgen AKD servo drive and three Leadshine EL7 servo
  drives

## Future goals

These may change at any time.

- [ ] Integration with LinuxCNC as a HAL component.
- [ ] A multiplatform configuration/debugging/management GUI

## Sponsors

Thank you to everyone who has donated test equipment, time or money to the EtherCrab project! To
help the EtherCrab project progress faster, please consider becoming a sponsor, or donating EtherCAT
hardware to ensure best compatibility.

- [@nealsjoe](https://twitter.com/nealsjoe) generously donated an EK1100 with several IO modules for
  testing with.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or
  http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.
