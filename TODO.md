- [x] Reset slave state before startup

  - [x] Minimum done, need to reset e.g. FMMUs and stuff
  - [ ] Read slave EEPROMs and reset using that info

- [ ] Rename `std` feature to `alloc` and only use the latter.
- [x] SII read
  - [x] Find strings section function
  - [x] Read name in chunks of however many bytes and form into a displayable string
- [ ] Experiment with using `MaybeUninit` for wakers again
- [ ] Find a way to split out the PDU TX/RX to ensure we only ever have one of them
- [x] Read FMMU and SM data from EEPROM
- [x] Use [embassy-futures](https://crates.io/crates/embassy-futures) for some things instead of
      smol or whatever I'm using now.
- [x] Optimise find string function to not use a buffer of 255 bytes on the stack
- [ ] Distributed clocks
- [ ] Find a way of storing PDUs in a single buffer instead of using a bunch of `heapless::Vec`s
- [ ] Byte-align each slave's PDI access for better safety
- [ ] Mailbox support for SDOs
  - [ ] Get and store mailbox lengths from EEPROM `Standard Receive Mailbox Offset `, etc, `0x0018`
  - [ ] If a slave has mailbox present, we need to read the PDO index/subindex out of
        rx_pdos/tx_pdos and their entries and use the mailbox to configure the PDOs. This might be
        why Ethercrab can't write to the LAN9252 outputs while SOEM can.
- [ ] Remove loop ticks from timeout checker loops. SOEM doesn't delay and neither should we I
      reckon. It's a spinloop, but it has async calls in it so potentially non-blocking.
- [ ] Revisit packed structs with confusing backwards bit orders. If `MailboxHeader` encodes on the
      wire correctly, I can use it's attributes elsewhere.
- [ ] PDI struct with read/write accessors or some other kind of slave interop
- [ ] Write a bunch of MIRI tests around PDI buffer read/write, and the PDU loop
