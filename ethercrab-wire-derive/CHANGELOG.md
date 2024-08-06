# Changelog

Derives for `ethercrab`.

<!-- next-header -->

## [Unreleased] - ReleaseDate

### Changed

- **(breaking)** [#230](https://github.com/ethercrab-rs/ethercrab/pull/230) Increase MSRV from 1.77
  to 1.79.

## [0.2.0] - 2024-07-28

## [0.1.4] - 2024-03-31

### Fixed

- [#207](https://github.com/ethercrab-rs/ethercrab/pull/207) Generate `EtherCrabWireSized` for
  write-only enums.

## [0.1.3] - 2024-03-27

## [0.1.2] - 2024-02-03

### Changed

- [#160](https://github.com/ethercrab-rs/ethercrab/pull/160) Packing buffers are now zeroed before
  being written into.

### Added

- [#159](https://github.com/ethercrab-rs/ethercrab/pull/159) Support `i*` enum discriminants. Also
  adds support for `u64`. `usize` and `isize` are explicitly unsupported as they can change size on
  different targets.

## [0.1.1] - 2024-01-11

## [0.1.0] - 2024-01-11

### Added

- Initial release

<!-- next-url -->
[unreleased]: https://github.com/ethercrab-rs/ethercrab/compare/ethercrab-wire-derive-v0.1.4...HEAD

[unreleased]: https://github.com/ethercrab-rs/ethercrab/compare/ethercrab-wire-derive-v0.2.0...HEAD
[0.2.0]:
  https://github.com/ethercrab-rs/ethercrab/compare/ethercrab-wire-derive-v0.1.4...ethercrab-wire-derive-v0.2.0
[0.1.4]:
  https://github.com/ethercrab-rs/ethercrab/compare/ethercrab-wire-derive-v0.1.3...ethercrab-wire-derive-v0.1.4
[0.1.3]:
  https://github.com/ethercrab-rs/ethercrab/compare/ethercrab-wire-derive-v0.1.2...ethercrab-wire-derive-v0.1.3
[0.1.2]:
  https://github.com/ethercrab-rs/ethercrab/compare/ethercrab-wire-derive-v0.1.1...ethercrab-wire-derive-v0.1.2
[0.1.1]:
  https://github.com/ethercrab-rs/ethercrab/compare/ethercrab-wire-derive-v0.1.0...ethercrab-wire-derive-v0.1.1
[0.1.0]: https://github.com/ethercrab-rs/ethercrab/compare/HEAD...ethercrab-wire-derive-v0.1.0
