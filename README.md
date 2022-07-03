# EtherCrab

An EtherCAT master written in pure Rust.

EtherCrab is looking for sponsors! I (@jamwaffles) am developing EtherCrab in my spare time with
currently no fulltime income, so if you want a safe, performant Rust EtherCat master please consider
becoming a sponsor!

## Current goals

- [x] Become a member of the
      [EtherCAT Technologies Group (ETG)](https://www.ethercat.org/default.htm) and get access to
      the EtherCAT specification.
- [ ] (in progress) Explore basic master architecture to support current design goals
- [ ] `async` API usable in no_std environments with either [RTIC](https://rtic.rs) (once async
      support is released) or [Embassy](https://embassy.dev/)
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

- [ ] A blocking API which spins on internal futures for best compatibility, possibly using
      [casette](https://lib.rs/crates/cassette) or [nb-executor](https://lib.rs/crates/nb-executor).
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
