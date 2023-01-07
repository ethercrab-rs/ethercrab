# Changelog

An EtherCAT master written in Rust.

<!-- next-header -->

## [Unreleased] - ReleaseDate

### Changed

- **(breaking)** [#1] `SlaveGroup::slaves` now returns an iterator over each slave with IO in the
  group, instead of a plain slave.
- **(breaking)** [#2] Rename `slave_group::Configurator` to `SlaveGroupRef`.

### Added

- [#1] Added `SlaveGroup::len` and `SlaveGroup::is_empty` methods.

## [0.1.0] - 2023-01-02

### Added

- Initial release

<!-- next-url -->

[#1]: https://github.com/ethercrab-rs/ethercrab/pull/1
[unreleased]: https://github.com/ethercrab-rs/ethercrab/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/ethercrab-rs/ethercrab/compare/fb37346...v0.1.0
