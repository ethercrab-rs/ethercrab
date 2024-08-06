# Changelog

Traits used for converting optionally packed values to/from raw data as represented in the EtherCAT
specification.

Primarily used by `ethercrab`.

<!-- next-header -->

## [Unreleased] - ReleaseDate

### Changed

- **(breaking)** [#230](https://github.com/ethercrab-rs/ethercrab/pull/230) Increase MSRV from 1.77
  to 1.79.

## [0.2.0] - 2024-07-28

### Changed

- **(breaking)** [#218](https://github.com/ethercrab-rs/ethercrab/pull/218) Removed `expected` and
  `got` fields from `WireError::{Read,Write}BufferTooShort`.
- **(breaking)** [#218](https://github.com/ethercrab-rs/ethercrab/pull/218) Increase MSRV from 1.75
  to 1.77.

## [0.1.4] - 2024-03-31

## [0.1.3] - 2024-03-27

### Added

- [#183](https://github.com/ethercrab-rs/ethercrab/pull/183) Add support for encoding/decoding
  tuples up to 16 items long.

## [0.1.2] - 2024-02-03

### Changed

- [#160](https://github.com/ethercrab-rs/ethercrab/pull/160) Packing buffers are now zeroed before
  being written into.

## [0.1.1] - 2024-01-11

## [0.1.0] - 2024-01-11

### Added

- Initial release

<!-- next-url -->

[unreleased]: https://github.com/ethercrab-rs/ethercrab/compare/ethercrab-wire-v0.2.0...HEAD
[0.2.0]:
  https://github.com/ethercrab-rs/ethercrab/compare/ethercrab-wire-v0.1.4...ethercrab-wire-v0.2.0
[0.1.4]:
  https://github.com/ethercrab-rs/ethercrab/compare/ethercrab-wire-v0.1.3...ethercrab-wire-v0.1.4
[0.1.3]:
  https://github.com/ethercrab-rs/ethercrab/compare/ethercrab-wire-v0.1.2...ethercrab-wire-v0.1.3
[0.1.2]:
  https://github.com/ethercrab-rs/ethercrab/compare/ethercrab-wire-v0.1.1...ethercrab-wire-v0.1.2
[0.1.1]:
  https://github.com/ethercrab-rs/ethercrab/compare/ethercrab-wire-v0.1.0...ethercrab-wire-v0.1.1
[0.1.0]: https://github.com/ethercrab-rs/ethercrab/compare/HEAD...ethercrab-wire-v0.1.0
