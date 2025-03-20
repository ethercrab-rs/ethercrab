# Changelog

An EtherCAT master written in Rust.

<!-- next-header -->

## [Unreleased] - ReleaseDate

## [0.5.4] - 2025-03-20

### Fixed

- [#288](https://github.com/ethercrab-rs/ethercrab/pull/288) Prevent overflow and infinite loops
  when reading empty or nearly-empty SubDevice EEPROMs.

## [0.5.3] - 2025-01-24

### Added

- [#242](https://github.com/ethercrab-rs/ethercrab/pull/242) Add support for PDIs longer than a
  single PDU

## [0.5.2] - 2024-12-21

### Removed

- **(breaking)** [#260](https://github.com/ethercrab-rs/ethercrab/pull/260) The `PduError::NoWaker`
  variant has been removed as it is no longer used or returned by any EtherCrab method.

### Fixed

- [#260](https://github.com/ethercrab-rs/ethercrab/pull/260) Frames received before their future's
  waker is registered will no longer trigger a `NoWaker` error.

## [0.5.1] - 2024-11-26

### Deprecated

- [#248](https://github.com/ethercrab-rs/ethercrab/pull/248) **Windows:** `tx_rx_task` is replaced
  with `tx_rx_task_blocking` which is no longer `async`. It must be spawned into its own thread
  instead of an async task. `tx_rx_task` will be removed in a future release.

### Added

- [#248](https://github.com/ethercrab-rs/ethercrab/pull/248) **Windows:** Add `tx_rx_task_blocking`
  to use in a separate thread to send/receive EitherCAT frames.

### Changed

- **(breaking)** [#248](https://github.com/ethercrab-rs/ethercrab/pull/248) `PduRx::receive_frame`
  now returns `Result<ReceiveAction, Error>` instead of `Result<(), Error>`.

## [0.5.0] - 2024-07-28

### Changed

- [#216](https://github.com/ethercrab-rs/ethercrab/pull/216) PDU retries now use the same PDU index
  instead of allocating a new frame for every resend.
- [#217](https://github.com/ethercrab-rs/ethercrab/pull/217) EEPROM strings must now be valid ASCII
  as per the EtherCAT specification. UTF-8 strings will return an error when read.
- **(breaking)** [#218](https://github.com/ethercrab-rs/ethercrab/pull/218) Removed `expected` and
  `got` fields from `Error::Wire(WireError::{Read,Write}BufferTooShort)`.
- **(breaking)** [#218](https://github.com/ethercrab-rs/ethercrab/pull/218) Increase MSRV from 1.75
  to 1.77.
- **(breaking)** [#227](https://github.com/ethercrab-rs/ethercrab/pull/227) Renamed many public
  items to use the newer EtherCAT terminology `MainDevice` and `SubDevice` for master/slave
  respectively.

  | Type      | Old                                        | New                                            |
  | --------- | ------------------------------------------ | ---------------------------------------------- |
  | `enum`    | `SlaveState`                               | `SubDeviceState`                               |
  | `fn`      | `Client::num_slaves()`                     | `MainDevice::num_subdevices()`                 |
  | `fn`      | `Ds402::slave()`                           | `Ds402::subdevice()`                           |
  | `fn`      | `SlaveGroup::slave()`                      | `SubDeviceGroup::subdevice()`                  |
  | `mod`     | `ethercrab::slave_group`                   | `ethercrab::subdevice_group`                   |
  | `struct`  | `Client`                                   | `MainDevice`                                   |
  | `struct`  | `ClientConfig`                             | `MainDeviceConfig`                             |
  | `struct`  | `GroupSlaveIterator`                       | `GroupSubDeviceIterator`                       |
  | `struct`  | `Slave`                                    | `SubDevice`                                    |
  | `struct`  | `SlaveGroup`                               | `SubDeviceGroup`                               |
  | `struct`  | `SlaveGroupRef`                            | `SubDeviceGroupRef`                            |
  | `struct`  | `SlaveIdentity`                            | `SubDeviceIdentity`                            |
  | `struct`  | `SlavePdi`                                 | `SubDevicePdi`                                 |
  | `struct`  | `SlaveRef`                                 | `SubDeviceRef`                                 |
  | `variant` | `AlStatusCode::SlaveNeedsColdStart`        | `AlStatusCode::SubDeviceNeedsColdStart`        |
  | `variant` | `AlStatusCode::SlaveNeedsInit`             | `AlStatusCode::SubDeviceNeedsInit`             |
  | `variant` | `AlStatusCode::SlaveNeedsPreop`            | `AlStatusCode::SubDeviceNeedsPreop`            |
  | `variant` | `AlStatusCode::SlaveNeedsRestartedLocally` | `AlStatusCode::SubDeviceNeedsRestartedLocally` |
  | `variant` | `AlStatusCode::SlaveNeedsSafeop`           | `AlStatusCode::SubDeviceNeedsSafeop`           |
  | `variant` | `Error::UnknownSlave`                      | `Error::UnknownSubDevice`                      |

### Added

- [#162](https://github.com/ethercrab-rs/ethercrab/pull/162) Add support for FreeBSD and NetBSD
  using BPF.

## [0.4.2] - 2024-05-27

### Added

- [#213](https://github.com/ethercrab-rs/ethercrab/pull/213) (@jbbjarnason) Add `alias_address`
  method to `Slave` and `SlaveRef` to get a SubDevice's alias address.

## [0.4.1] - 2024-04-05

### Fixed

- [#208](https://github.com/ethercrab-rs/ethercrab/pull/208) Expose `DcSupport` enum at the crate
  root so it can be used by everyone.

## [0.4.0] - 2024-03-31

### Changed

- **(breaking)** [#134](https://github.com/ethercrab-rs/ethercrab/pull/134) Bump MSRV to 1.75.0
- [#134](https://github.com/ethercrab-rs/ethercrab/pull/134) Refactor sub device EEPROM reader to be
  more efficient when skipping sections of the device EEPROM map.
- **(breaking)** [#142](https://github.com/ethercrab-rs/ethercrab/pull/142) Remove `PduRead` and
  `PduData` traits. These are replaced with `EtherCrabWireRead` and `EtherCrabWireReadWrite` traits
  respectively, along with `EtherCrabWireReadWrite` for write-only items.

  Some pertinent trait bounds changes in the public API:

  - `SlaveRef::sdo_read` from `PduData` to `EtherCrabWireWrite`
  - `SlaveRef::sdo_write` from `PduData` to `EtherCrabWireReadSized`
  - `SlaveRef::register_read` from `PduData` to `EtherCrabWireWrite`
  - `SlaveRef::register_write` from `PduData` to `EtherCrabWireReadWrite`

- **(breaking)** [#144](https://github.com/ethercrab-rs/ethercrab/pull/144)
  `PduError::InvalidIndex(usize)` is now a `PduError::InvalidIndex(u8)` as the EtherCAT index field
  is itself onl a `u8`.
- [#151](https://github.com/ethercrab-rs/ethercrab/pull/151) Reduced overhead for EEPROM reads. Each
  chunk reader now only checks for and (attempt to) clear device errors once before reading a chunk
  of data, not for every chunk.
- [#156](https://github.com/ethercrab-rs/ethercrab/pull/156) Update `embassy-time` from 0.2.0 to
  0.3.0.
- [#181](https://github.com/ethercrab-rs/ethercrab/pull/181) `PduStorage` now stores complete
  Ethernet frames instead of building them on the fly. This adds a little more overhead to each
  slot, so the reserved data const parameter must be larger to compensate. Use the new
  `PduStorage::element_size` method to calculate element sizes based on a given maximum PDU payload
  value.

### Added

- [#141](https://github.com/ethercrab-rs/ethercrab/pull/141) Added the `ethercat-wire` and
  `ethercat-wire-derive` crates.

  These crates are **EXPERIMENTAL**. They may be improved for public use in the future but are
  currently designed around EtherCrab's internal needs and may be rough and/or buggy. Use with
  caution, and expect breaking changes.

- [#141](https://github.com/ethercrab-rs/ethercrab/pull/141) Re-export the following traits from
  `ethercrab-wire` for dealing with packing/unpacking data:

  - `EtherCrabWireRead`
  - `EtherCrabWireReadSized`
  - `EtherCrabWireReadWrite`
  - `EtherCrabWireSized`
  - `EtherCrabWireWrite`

- [#151](https://github.com/ethercrab-rs/ethercrab/pull/151) Add `EepromError::ClearErrors` variant.
- [#152](https://github.com/ethercrab-rs/ethercrab/pull/152) Expose `error::CoeAbortCode` for
  matching on CoE transfer errors.
- [#169](https://github.com/ethercrab-rs/ethercrab/pull/169) Linux only: add `io_uring`-based
  blocking TX/RX loop for better performance.
- [#173](https://github.com/ethercrab-rs/ethercrab/pull/173) Add MUSL libc support.
- [#178](https://github.com/ethercrab-rs/ethercrab/pull/178) Add `Error::SubDevice` to get a
  subdevice status code on failure.
- [#180](https://github.com/ethercrab-rs/ethercrab/pull/#180) Add `PreOpPdi` state, allowing access
  to a group's PDI whilst in `PRE-OP`.
- [#180](https://github.com/ethercrab-rs/ethercrab/pull/#180) Add `ethercrab::std::ethercat_now`
  function to get the current time in nanoseconds from the EtherCAT epoch of 2000-01-01.
- [#194](https://github.com/ethercrab-rs/ethercrab/pull/#194) Added `SlaveGroup` methods to
  facilitate graceful shutdown:

  - `SlaveGroup<Op>::into_safe_op`
  - `SlaveGroup<SafeOp>::into_pre_op`
  - `SlaveGroup<PreOp>::into_init`

  The `ek1100` example shows these methods in use.

- [#195](https://github.com/ethercrab-rs/ethercrab/pull/#195) Add `Register::DcCyclicUnitControl`
  (0x0980).
- [#193](https://github.com/ethercrab-rs/ethercrab/pull/#193) Add
  `SlaveGroup::<PreOp>::request_into_op` to request all SubDevices in a group transition to OP, but
  does not wait for them to transition. Also add `SlaveGroup::<Op>::all_op` to check if all
  SubDevices in the group have reached OP state.
- [#198](https://github.com/ethercrab-rs/ethercrab/pull/#198) Add `Error::DistributedClock(_)` and
  `DistributedClockError` error variant and type to communicate DC errors.
- [#198](https://github.com/ethercrab-rs/ethercrab/pull/#198) Add
  `SlaveGroup::tx_rx_sync_system_time`, `SlaveGroup::tx_rx_dc`, `SlaveRef::set_dc_sync` and
  `SlaveGroup::configure_dc_sync` to support EtherCAT Distributed Clocks.

### Fixed

- **(breaking)** (technically) [#143](https://github.com/ethercrab-rs/ethercrab/pull/143) Fix typo
  in name `AlStatusCode::ApplicationControllerAvailableI` ->
  `AlStatusCode::ApplicationControllerAvailable`
- [#152](https://github.com/ethercrab-rs/ethercrab/pull/152) CoE errors are not reported correctly
  from `sdo_read` and `sdo_write`.
- [#194](https://github.com/ethercrab-rs/ethercrab/pull/194) `SlaveGroup<PreOp>::into_op` now
  transitions through SAFE-OP instead of illegally transitioning straight into OP.

### Removed

- **(breaking)** [#145](https://github.com/ethercrab-rs/ethercrab/pull/145) Remove the `context`
  field from `Error::WorkingCounter`. The output from EtherCrab's error logging should be used
  instead.
- **(breaking)** [#181](https://github.com/ethercrab-rs/ethercrab/pull/181) Remove async
  `SendableFrame::send`. Use `SendableFrame::send_blocking` instead.
- [#197](https://github.com/ethercrab-rs/ethercrab/pull/197) Remove `SlaveGroupState` trait. It is
  no longer required, but the same methods are available so migration should be as simple as just
  removing the import.

## [0.3.7] - 2024-03-12

### Fixed

- [#183](https://github.com/ethercrab-rs/ethercrab/pull/183) Relax `'static` bound for `tx_rx_task`
  on Windows.

## [0.3.6] - 2024-02-14

### Added

- [#167](https://github.com/ethercrab-rs/ethercrab/pull/167) Add support for reading/writing `f32`,
  `f64` and `bool`. Note that `f64` cannot currently be written using `sdo_write` as only 4 byte
  expedited transfers are currently supported.

## [0.3.5] - 2023-12-22

### Changed

- [#135](https://github.com/ethercrab-rs/ethercrab/pull/135) macOS only: `tx_rx_task` now uses
  native networking (BPF) instead of `libpcapng` to improve reliability.
- **(breaking)** [#136](https://github.com/ethercrab-rs/ethercrab/pull/136) Fix unsoundness issue
  where `SlaveRef::io_raw` could be called multiple times, allowing multiple mutable references into
  the device's output data.
- **(breaking)** [#136](https://github.com/ethercrab-rs/ethercrab/pull/136) Rename
  `SlaveRef::io_raw` to `SlaveRef::io_raw_mut`. `SlaveRef::io_raw` remains, but now only returns
  non-mutable references to both the device inputs and outputs.

  Also renames `SlaveRef::outputs_raw` to `SlaveRef::outputs_raw_mut`. `SlaveRef::outputs` now
  returns a non-mutable reference to the device output data.

## [0.3.4] - 2023-11-20

### Fixed

- [#132](https://github.com/ethercrab-rs/ethercrab/pull/132) The mailbox counter is now per-device
  instead of global, fixing issues with many devices communicating over CoE.

### Changed

- [#132](https://github.com/ethercrab-rs/ethercrab/pull/132) Revert
  [#130](https://github.com/ethercrab-rs/ethercrab/pull/130) "Counter in mailbox response is no
  longer checked." as this was masking the root cause, which is now fixed.
- **(breaking)** [#132](https://github.com/ethercrab-rs/ethercrab/pull/132) `Slave` no longer
  implements `Clone` or `PartialEq`. Devices should instead be compared using `name()`,
  `identity()`, `configured_address()`, etc.

## [0.3.3] - 2023-11-10

### Changed

- [#130](https://github.com/ethercrab-rs/ethercrab/pull/130) Counter in mailbox response is no
  longer checked.

## [0.3.2] - 2023-11-02

### Added

- [#122] Added `Slave{Ref}::propagation_delay()` to get the EtherCAT propagation delay for a
  specific device on the network.
- [#126] Implement `PduRead` and `PduData` for `[u8; N]`.

### Fixed

- [#121] **Linux only:** Relax `'static` lifetime requirement on `std::tx_rx_task` to a named
  lifetime to allow non-`'static` storage to be used.
- [#124] Fixed some spurious panics from race conditions by using atomic wakers.
- [#127] Improve frame allocation reliability when contention is high.

### Changed

- **(breaking)** [#124] Changed `PduTx::waker()` to `PduTx::replace_waker()`. Instead of calling
  e.g. `pdu_tx.waker().replace(ctx.waker().clone())`, now it should be
  `pdu_tx.replace_waker(ctx.waker())`.
- (potentially breaking) [#125] Package upgrades, notably `async_io` and `futures_lite` from 1.x to
  2.0.

## [0.3.1] - 2023-10-16

## [0.3.0] - 2023-10-12

### Added

- [#91] Add support for "cross" topologies, e.g. with EK1122.
- [#102] PDU retry behaviour is now configurable between no retries, a limited count, or retrying
  forever with the `RetryBehaviour` struct and associated `ClientConfig.retry_behaviour` option.
- [#103] Added optional `serde` feature to enable ser/de of some EtherCrab items.
- [#104] Implement `std::error::Error` for `ethercrab::error::Error` when the `std` feature is
  enabled.
- [#107] Add watchdog fields to `Register` enum: `WatchdogDivider`, `PdiWatchdog`
  `SyncManagerWatchdog`, `SyncManagerWatchdogStatus` `SyncManagerWatchdogCounter`,
  `PdiWatchdogCounter`.
- [#113] `SlaveState` now implements `PduRead` so can now be read directly, e.g.

  ```rust
  let (status, _wkc) =
      Command::fprd(slave.configured_address(), RegisterAddress::AlStatus.into())
          .receive::<SlaveState>(&client)
          .await?;
  ```

### Changed

- [#92] If no slave devices are detected, `Client::init` will no longer exit with an error.
- **(breaking)** [#101] `SendableFrame::send_blocking` and `SendableFrame::send` must now return the
  number of bytes sent over the network.
- **(breaking)** [#101] `SendableFrame::write_ethernet_packet` is no longer `pub`. Instead, use
  `SendableFrame::send_blocking` or `SendableFrame::send`.
- [#103] Removed inner `smoltcp::error::Error` from `PduError::Ethernet` and `PduError::CreateFrame`
  as these don't add much meaning to the variant.
- **(breaking)** [#109] Make all methods on `PduLoop` private.
- **(breaking)** [#113] `Command::{code,address,parse}` are no longer `pub`.
- **(breaking)** [#119] Changed `SlaveState::Unknown` to `SlaveState::Other(u8)` to better represent
  unknown or different states of multiple slaves (e.g. when sending a `BRD`).

### Removed

- **(breaking)** [#99] All PDU methods on `Client` (`Client::bwr`, `Client::fprd`) have been
  removed. Instead, use the same methods on `Command` like `Command::bwr`, `Command::fprd` etc.

## [0.2.1] - 2023-07-31

### Fixed

- [#84] `GroupSlave::iter` will now panic instead of completing early if a slave device is already
  borrowed.
- [#114] The `std` TX/RX future now consumes any queued packets, not just the first one. This fixes
  PDU timeout issues with `zip`/`join`ed futures.

### Added

- [#83] Add `SlaveRef::identity` method to get the vendor ID, hardware revision, etc of a slave
  device.
- [#86] Expose the `SlaveIdentity` struct.

### Changed

- [#84] The `SlaveGroupState` trait is now not-doc-hidden so the `GroupSlave::slave` method is more
  easily accessible.

## [0.2.0] - 2023-07-31

### Added

- [#47] Add the ability to read/write registers/SDOs from grouped slave devices, with the methods
  `SlaveRef::register_read`, `SlaveRef::register_write`, `SlaveRef::sdo_read` and
  `SlaveRef::sdo_write`.
- [#30] Added `Copy`, `Clone`, `PartialEq` and `Eq` implementations to `Error` and `PduError`.
- [#1] Added `SlaveGroup::len` and `SlaveGroup::is_empty` methods.
- [#29] Implement `Display` for `Error`, `PduError`, `MailboxError`, `EepromError`,
  `VisibleStringError` and `PduValidationError`
- **(breaking)** [#31] Added a `ClientConfig` argument to `Client::new` to allow configuration of
  various EtherCrab behaviours.
- [#55] Added `Client::init_single_group` to reduce boilerplate when only using a single group of
  devices.
- [#55] Removed MSRV commitment (was 1.68)
- [#59] Added `SendableFrame::send_blocking` method.

### Removed

- **(breaking)** [#75] `Client::request_slave_state` is removed. Groups should be transitioned into
  the various states individually using `into_op` or `into_safe_op`.
- **(breaking)** [#75] `SlaveGroup::new` is removed. Slave groups can be created with
  `SlaveGroup::default()` instead
- **(breaking)** [#45] The `SlaveGroupContainer` trait is no longer needed and has been removed.

### Changed

- **(breaking)** [#75] `Client::init` no longer takes a `groups` argument and now requires
  `G: Default`.
- **(breaking)** [#75] `SlaveGroup`s no longer configure using a closure - instead use
  `SlaveGroup::iter` or `SlaveGroup::slave` to configure slave devices inline.
- **(breaking)** [#75] `SlaveGroup`s now have a type state. Use `into_safe_op` and `into_op` to
  transition from PRE-OP as provided by `Client::init` into run mode.
- [#47] Slave `sdo_read` and `sdo_write` methods no longer require the use of `SubIndex`. For single
  accesses, a raw `u8` can be passed instead for cleaner configuration code.
- **(breaking)** [#47] `SlaveGroup::slave` and `SlaveGroup::iter` (was `slaves`) now requires the
  passing of a `Client` reference when called.
- **(breaking)** [#47] `SlaveGroup::slaves` is renamed to `SlaveGroup::iter`
- **(breaking)** [#47] Grouped slaves that were previously represented as `GroupSlave`s are now
  represented as `SlaveRef<'_, SlavePdi<'_>>` instead. `GroupSlave` is removed.
- **(breaking)** [#47] The `io()`, `inputs()` and `outputs()` methods on grouped slaves have been
  renamed to `io_raw()`, `inputs_raw()` and `outputs_raw()` respecitively.
- **(breaking)** [#47] The `Slave.name` and `Slave.identity` fields have been replaced with methods
  of the same name.
- **(breaking)** [#45] The grouping closure passed to `Client::init` now requires a
  `&dyn SlaveGroupHandle` to be returned. This is a sealed trait only implemented for `SlaveGroup`s
  and allows some internal refactors by erasing the const generics from `SlaveGroup`.
- **(breaking)** [#32] To mitigate some internal issues, `PduStorage` now requires `N` (the number
  of storage elements) to be a power of two.
- **(breaking)** [#33] `send_frames_blocking` is removed. It is replaced with
  `PduTx::next_sendable_frame` which can be used to send any available frames in a loop until it
  returns `None`.
- **(breaking)** [#30] Removed `PduError::Encode` variant.
- **(breaking)** [#25] Renamed `pdu_rx` to `receive_frame` to mirror `send_frames_blocking`.
- **(breaking)** [#20] Changed the way the client, tx and rx instances are initialised to only allow
  one TX and RX to exist.

  Before

  ```rust
  static PDU_STORAGE: PduStorage<MAX_FRAMES, MAX_PDU_DATA> = PduStorage::new();
  static PDU_LOOP: PduLoop = PduLoop::new(PDU_STORAGE.as_ref());

  async fn main_app(ex: &LocalExecutor<'static>) -> Result<(), Error> {
      let client = Arc::new(Client::new(&PDU_LOOP, Timeouts::default()));

      ex.spawn(tx_rx_task(INTERFACE, &client).unwrap()).detach();
  }
  ```

  After

  ```rust
  static PDU_STORAGE: PduStorage<MAX_FRAMES, MAX_PDU_DATA> = PduStorage::new();

  async fn main_app(ex: &LocalExecutor<'static>) -> Result<(), Error> {
      let (tx, rx, pdu_loop) = PDU_STORAGE.try_split().expect("can only split once");

      let client = Arc::new(Client::new(pdu_loop, Timeouts::default()));

      ex.spawn(tx_rx_task(INTERFACE, tx, rx).unwrap()).detach();
  }
  ```

- **(breaking)** [#16] Remove `TIMER`/`TIMEOUT` generic parameter. `std` environments will now use
  the timer provided by `smol` (`async-io`). `no_std` environments will use `embassy-time`.
- **(breaking)** [#9] Rename the fields of some variants in `ethercrab::error::Error` to make them
  less confusing.
- **(breaking)** [#2] Rename `slave_group::Configurator` to `SlaveGroupRef`.
- **(breaking)** [#1] `SlaveGroup::slaves` now returns an iterator over each slave with IO in the
  group, instead of a plain slave.

### Fixed

- [#28] Fix abort code parsing for expedited SDO responses.
- [#26] :tada: EtherCrab now works on stable Rust. The MSRV is 1.68.
- [#23] Strip trailing null bytes (`\0`) in strings read from SII
- [#14] Fixed various overflow/arithmetic bugs in distributed clock time calculations and PDI
  configuration
- [#6] Fixed topology detection not stopping at first upstream fork when there is a slave device
  before the fork.
- [#6] Internal bugfixes to topology discovery code.
- [#2] Fixed multiple group PDI mapping calculation during initialisation.
- [#57] Fixed a buffer size calculation crash when reading SDOs.

## [0.1.0] - 2023-01-02

### Added

- Initial release

<!-- next-url -->
[unreleased]: https://github.com/ethercrab-rs/ethercrab/compare/ethercrab-v0.5.4...HEAD

[0.5.4]: https://github.com/ethercrab-rs/ethercrab/compare/ethercrab-v0.5.3...ethercrab-v0.5.4
[0.5.3]: https://github.com/ethercrab-rs/ethercrab/compare/ethercrab-v0.5.2...ethercrab-v0.5.3
[0.5.2]: https://github.com/ethercrab-rs/ethercrab/compare/ethercrab-v0.5.1...ethercrab-v0.5.2
[0.5.1]: https://github.com/ethercrab-rs/ethercrab/compare/ethercrab-v0.5.0...ethercrab-v0.5.1
[0.5.0]: https://github.com/ethercrab-rs/ethercrab/compare/ethercrab-v0.4.2...ethercrab-v0.5.0
[0.4.2]: https://github.com/ethercrab-rs/ethercrab/compare/ethercrab-v0.4.1...ethercrab-v0.4.2
[0.4.1]: https://github.com/ethercrab-rs/ethercrab/compare/ethercrab-v0.4.0...ethercrab-v0.4.1
[0.4.0]: https://github.com/ethercrab-rs/ethercrab/compare/v0.3.6...ethercrab-v0.4.0
[0.3.7]: https://github.com/ethercrab-rs/ethercrab/compare/v0.3.6...v0.3.7
[0.3.6]: https://github.com/ethercrab-rs/ethercrab/compare/v0.3.5...v0.3.6
[0.3.5]: https://github.com/ethercrab-rs/ethercrab/compare/v0.3.4...v0.3.5
[0.3.4]: https://github.com/ethercrab-rs/ethercrab/compare/v0.3.3...v0.3.4
[0.3.3]: https://github.com/ethercrab-rs/ethercrab/compare/v0.3.2...v0.3.3
[0.3.4]: https://github.com/ethercrab-rs/ethercrab/compare/v0.3.2...v0.3.4
[0.3.2]: https://github.com/ethercrab-rs/ethercrab/compare/v0.3.0...v0.3.2
[0.3.1]: https://github.com/ethercrab-rs/ethercrab/compare/v0.3.0...v0.3.1
[0.3.0]: https://github.com/ethercrab-rs/ethercrab/compare/v0.2.1...v0.3.0
[0.2.1]: https://github.com/ethercrab-rs/ethercrab/compare/v0.2.0...v0.2.1
[#2]: https://github.com/ethercrab-rs/ethercrab/pull/2
[#1]: https://github.com/ethercrab-rs/ethercrab/pull/1
[#6]: https://github.com/ethercrab-rs/ethercrab/pull/6
[#9]: https://github.com/ethercrab-rs/ethercrab/pull/9
[#14]: https://github.com/ethercrab-rs/ethercrab/pull/14
[#16]: https://github.com/ethercrab-rs/ethercrab/pull/16
[#23]: https://github.com/ethercrab-rs/ethercrab/pull/23
[#20]: https://github.com/ethercrab-rs/ethercrab/pull/20
[#25]: https://github.com/ethercrab-rs/ethercrab/pull/25
[#26]: https://github.com/ethercrab-rs/ethercrab/pull/26
[#28]: https://github.com/ethercrab-rs/ethercrab/pull/28
[#29]: https://github.com/ethercrab-rs/ethercrab/pull/29
[#30]: https://github.com/ethercrab-rs/ethercrab/pull/30
[#31]: https://github.com/ethercrab-rs/ethercrab/pull/31
[#32]: https://github.com/ethercrab-rs/ethercrab/pull/32
[#33]: https://github.com/ethercrab-rs/ethercrab/pull/33
[#45]: https://github.com/ethercrab-rs/ethercrab/pull/45
[#47]: https://github.com/ethercrab-rs/ethercrab/pull/47
[#55]: https://github.com/ethercrab-rs/ethercrab/pull/55
[#59]: https://github.com/ethercrab-rs/ethercrab/pull/59
[#75]: https://github.com/ethercrab-rs/ethercrab/pull/75
[#83]: https://github.com/ethercrab-rs/ethercrab/pull/83
[#84]: https://github.com/ethercrab-rs/ethercrab/pull/84
[#86]: https://github.com/ethercrab-rs/ethercrab/pull/86
[#91]: https://github.com/ethercrab-rs/ethercrab/pull/91
[#92]: https://github.com/ethercrab-rs/ethercrab/pull/92
[#99]: https://github.com/ethercrab-rs/ethercrab/pull/99
[#101]: https://github.com/ethercrab-rs/ethercrab/pull/101
[#102]: https://github.com/ethercrab-rs/ethercrab/pull/102
[#103]: https://github.com/ethercrab-rs/ethercrab/pull/103
[#104]: https://github.com/ethercrab-rs/ethercrab/pull/104
[#107]: https://github.com/ethercrab-rs/ethercrab/pull/107
[#109]: https://github.com/ethercrab-rs/ethercrab/pull/109
[#113]: https://github.com/ethercrab-rs/ethercrab/pull/113
[#114]: https://github.com/ethercrab-rs/ethercrab/pull/114
[#119]: https://github.com/ethercrab-rs/ethercrab/pull/119
[#121]: https://github.com/ethercrab-rs/ethercrab/pull/121
[#122]: https://github.com/ethercrab-rs/ethercrab/pull/122
[#124]: https://github.com/ethercrab-rs/ethercrab/pull/124
[#125]: https://github.com/ethercrab-rs/ethercrab/pull/125
[#126]: https://github.com/ethercrab-rs/ethercrab/pull/126
[#127]: https://github.com/ethercrab-rs/ethercrab/pull/127
[0.2.0]: https://github.com/ethercrab-rs/ethercrab/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/ethercrab-rs/ethercrab/compare/fb37346...v0.1.0
