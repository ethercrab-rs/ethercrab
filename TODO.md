- [x] Moving slaves into client_inner
- [ ] Also need to move all the services into client_inner too
- [x] Make `frames` and `send_waker` un-pub
- [-] Turn slaves list from `RefCell<Vec<>>` into `Vec<RefCell<>>`? Would allow different tasks to
  borrow different slaves.

  - No; each refcell adds 2 bytes which is a lot to pay for.
  - Actually yes; needed for concurrent slave access

- [x] defmt logging support
- [x] Reset slave state before startup
  - [ ] Minimum done, need to reset e.g. FMMUs and stuff
- [x] Get any slave into PRE-OP state

  State request should send state request with PDU confirm, then spinloop (with an await) reading of
  AL status until timeout occurs

  - Set state to pre-op
  - Start spamming outputs to 0
  - Request state to safe-op
  - Request state to op

- [ ] Rename `std` feature to `alloc` and only use the latter.
- [x] Dedupe some of the code in all the service methods in client_inner
- [x] SII read manufacturer
  - [x] General EEPROM read functionality
    - Clear SII errors
    - Check if busy (with timeouted poll loop)
    - Send read operation
    - Wait until busy flag is cleared - the data should then be available in the data register
    - Read 32 bits from SII data address
  - [ ] Find strings section function
  - [ ] Read name in chunks of however many bytes and form into a displayable string
- [x] Replace wakers and frames with `[UnsafeCell<Option<Thingy>>]`
  - `UnsafeCell` has no memory overhead
  - Where `Thingy` is the waker, the task state enum, and the data array
  - [ ] Run under Miri with a few threads or something to see if there are weird concurrency issues
- [ ] Add methods to put all slaves into same state using a BWR
