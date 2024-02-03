# Changelog

Derives for `ethercrab`.

<!-- next-header -->

## [Unreleased] - ReleaseDate

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
[unreleased]: https://github.com/ethercrab-rs/ethercrab/compare/ethercrab-wire-derive-v0.1.2...HEAD

[0.1.2]: https://github.com/ethercrab-rs/ethercrab/compare/ethercrab-wire-derive-v0.1.1...ethercrab-wire-derive-v0.1.2
[0.1.1]:
  https://github.com/ethercrab-rs/ethercrab/compare/ethercrab-wire-derive-v0.1.0...ethercrab-wire-derive-v0.1.1
[0.1.0]: https://github.com/ethercrab-rs/ethercrab/compare/HEAD...ethercrab-wire-derive-v0.1.0
