- [x] Reset slave state before startup

  - [x] Minimum done, need to reset e.g. FMMUs and stuff
  - [ ] Read slave EEPROMs and reset using that info

- [ ] Rename `std` feature to `alloc` and only use the latter.
- [x] SII read
  - [ ] Find strings section function
  - [ ] Read name in chunks of however many bytes and form into a displayable string
- [ ] Experiment with using `MaybeUninit` for wakers again
- [ ] Find a way to split out the PDU TX/RX to ensure we only ever have one of them
- [ ] Read FMMU and SM data from EEPROM
- [x] Use [embassy-futures](https://crates.io/crates/embassy-futures) for some things instead of
      smol or whatever I'm using now.
- [ ] Experiment with `Cell::as_slice_of_cells`
