# `ethercrab-wire`

[![Build Status](https://circleci.com/gh/ethercrab-rs/ethercrab/tree/master.svg?style=shield)](https://circleci.com/gh/ethercrab-rs/ethercrab/tree/master)
[![Crates.io](https://img.shields.io/crates/v/ethercrab-wire.svg)](https://crates.io/crates/ethercrab-wire)
[![Docs.rs](https://docs.rs/ethercrab-wire/badge.svg)](https://docs.rs/ethercrab-wire)

Traits used to pack/unpack structs and enums from EtherCAT packets on the wire.

This crate is designed for use with [`ethercrab`](https://docs.rs/ethercrab) but can be
used standalone too.

While these traits can be implemented by hand as normal, it is recommended to derive them using
[`ethercrab-wire-derive`](https://docs.rs/ethercrab-wire-derive) where possible.

## Experimental

This crate is in its early stages and may contain bugs or publish breaking changes at any time.
It is in use by [`ethercrab`](https://docs.rs/ethercrab) and is well exercised there,
but please use with caution in your own code.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or
  http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.
