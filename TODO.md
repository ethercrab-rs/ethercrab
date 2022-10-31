- [x] Reset slave state before startup

  - [x] Minimum done, need to reset e.g. FMMUs and stuff
  - [-] Read slave EEPROMs and reset using that info
    - Don't need to. It's fine to just write zeroes to the entire memory range.

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
- [x] **Distributed clocks**
  - [x] Figure out topology from 0x0110/0x0111 and support more than naive in -> out
- [ ] Find a way of storing PDUs in a single buffer instead of using a bunch of `heapless::Vec`s
- [x] Byte-align each slave's PDI access for better safety
- [x] Mailbox support for SDOs

  - [x] Get and store mailbox lengths from EEPROM `Standard Receive Mailbox Offset `, etc, `0x0018`
  - [-] If a slave has mailbox present, we need to read the PDO index/subindex out of
    rx_pdos/tx_pdos and their entries and use the mailbox to configure the PDOs. This might be why
    Ethercrab can't write to the LAN9252 outputs while SOEM can.

    Hmm maybe not. LAN9252 works now. I think for e.g. AKD config it needs to have the CANOpen
    objects configured in the PO2SO hook, then the PDOs read not from eeprom but [where?].

- [x] Remove loop ticks from timeout checker loops. SOEM doesn't delay and neither should we I
      reckon. It's a spinloop, but it has async calls in it so potentially non-blocking.
  - Loop tick is now configurable globally and defaults to zero but can be increased if desired
- [ ] Revisit packed structs with confusing backwards bit orders. If `MailboxHeader` encodes on the
      wire correctly, I can use it's attributes elsewhere.
- [ ] Refactor code so we can drive various parts of EtherCrab with MIRI
  - [ ] PDU loop
  - [ ] Slave group
  - [ ] Check for other `unsafe` and test that too
- [x] Group support
- [-] Refactor FMMU mapping to group Is and Os for groups sequentially
  - Why? Each slave group will send its entire PDI anyway, as well as store each slave's PDI range
    in a list.
- [x] Make pdu_loop accept mutable slice references so we don't copy so much data around
  - [x] Also allows creation of `PduLoopRef` which will (hopefully) elide all the const generics,
        making passing it around much cleaner - likely with just a lifetime.
- [x] Extremely basic AKD initialisation:
  - Write 0x00 to 0x1c12:00
  - Write 0x1701 to 0x1c12:01
  - Write 0x01 to 0x1c12:02
- [ ] Benchmarks
  - Look into Iai <https://github.com/bheisler/iai>
- [ ] Create an Element room and put the link in the README
- [ ] Support networks with slaves that don't support distributed clocks. This will likely need test
      hardware to get working properly.
- [x] Make inputs and outputs contiguous in PDI so we can just update inputs from response and not
      clobber the outputs as currently occurs.
- [ ] Individual timeouts for ECSM transitions
  - TwinCAT shows it like this:
    - I -> P: 3000,
    - P -> S, S -> O: 10000
    - Back to P, I: 5000,
    - O -> S: 200
- [ ] EL3004 fails to initialise with
      `thread 'main' panicked at 'Init: NotFound { item: Fmmu, index: None }', examples\dc.rs:55:10`.
      Same with EL3204 and presumably EL3202. Works fine with EL2828 and EL1018
- [ ] Figure out why LAN9252 doesn't like showing any output. Inputs work fine so maybe a DC sync
      issue? Idk.
