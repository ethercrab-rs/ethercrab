# Plotting DC/OS time sync

```gnuplot
set datafile separator ','
# set xdata time # tells gnuplot the x axis is time data
# set ylabel "First Y " # label for the Y axis
set autoscale fix
set key top right outside autotitle columnhead
set xlabel 'OS time (ns)' # label for the X axis

set y2tics # enable second axis
set ytics nomirror # dont show the tics on that side
set y2label "OS/ECAT delta" # label for second axis

# plot './dc-pi.csv' using 1:3 title "Difference" with lines, '' using 1:4 title "Offset" with lines, '' using 1:5 title "PI out" with lines
plot './dc-pi.csv' using 1:4 title "Offset" with lines, '' using 1:3 title "OS/ECAT delta" with lines axis x1y2
```

# Optimising PDU reservation

Problem

- Must not be able to reuse a PDU index already in flight. Really bad data corruption could occur
  even when checking command code, etc. E.g. Cyclic LRW is same command code over and over again and
  would corrupt silently if PDU indices aren't reserved.

Solutions

- Current one: Array of `AtomicU16`
  - Memory inefficient
  - Slow to free reserved PDUs
- Possible: head and tail `AtomicU8`
  - Before reserving a new PDU index, check if tail has reached head. If it has, error out - we're
    sending stuff too fast, or not releasing it fast enough on the other end.
  - Each frame keeps the index of the first PDU pushed into it. This is unique
  - Isn't this a performance issue as it requires earlier PDUs to be freed before newer ones are?
    Yes it is. Frames can (although usually don't tbf) arrive in any order.
  - What happens if PDU indices within a single frame are not contiguous?
- Possible: store an array of PDU indices in each frame element
  - This is all internal, so I have control over how many items to have per element.
  - Find frame by first PDU in list as usual
  - How do we make this as small as possible in memory? `[u8; N]` can't represent unused state.
    `heapless::Vec` would work but it contains a `usize` which is a little large. If we have 4
    frames, `usize + [u8; 4]` is the same as `[u16; 4]` on 32 bit.
  - We already have a refcount, so we could just do `[u8; N]`.
  - On RX, have to loop through frames to find one with first PDU index equal to what was received.
    Bummer.

# Multiple PDUs per frame (again)

- One waker per frame
- A frame is received from the network
  - Multiple PDUs in the frame
  - Write their responses back into the buffer in a loop
  - When this is all done, all PDUs are ready to be used
  - Wait on the FRAME, not any PDU. Should be pretty much the same code as today
- This can be a safe but fragile API as it's just internal
- Submit a PDU with (data, len override, more follows, etc whatever)

  - Get back a range into the response frame to drag the data out of
  - Range can also be used to get wkc (just `end.. (end+2)`).

  ```rust
  let mut frame = pdu_loop.allocate_frame();

  let sub1 = frame.submit(Command::FRMW, 0u64, None, true);
  let sub2 = frame.submit(Command::LRW, &pdi, None, false);

  let result = frame.mark_sent().await;

  let res1 = result.take(sub1);
  let res2 = result.take(sub2);
  ```

- Don't need to store a refcount. If you want the frame data back you gotta `take()` it while
  `result` is still around.

- Same problem as before: multiple PDUs within the frame contain the index, but the receiving frame
  itself (which holds the waker) has no way of matching up what was sent. This means that even with
  a single waker, this is hard to fix.
  - One possible solution would be to store a sum/hash/`Range<>` of the indices in the PDUs and
    match against that, but that requires parsing the entire frame first.
- Another same problem as before is making sure we only have one of each PDU index in flight.

  Some solutions:

  1. Keep an array of PDU statuses that index back to the in-flight frame.

     - A map, essentially, where array index is PDU index, and the value in that cell is the frame
       index
     - Don't need to store anything else because we're staying with the single future per frame
       thing
     - Types
       - `[u8; 256]`? How do we represent "available" state?
       - `[u16; 256]`? A bit wasteful but we have a whole byte to play with for state bits. Only 512
         bytes or a third the nomal MTU so not awful tbh...
       - `[AtomicU16; 256]` for thread safety. Don't have to mess around with `mut`. `u16::MAX` can
         be "not occupied" sentinel.
     - When we try to reserve an index in `CreatedFrame::submit()`, if its slot is occupied, error
       out - stuff is either too small or too fast.
     - When a frame is received, we parse the first PDU header, look its index up in the array, and
       match that through to the frame index, then `claim_receiving`.
       - We should `assert` every PDU in that same frame after has the same frame index because it's
         a logic bug if it isn't, but in prod we can assume they're the same.
       - Can't have multiple receiving claims either, so it's ok to assume that all PDUs have the
         same frame index.
     - What happens to the index mappings when the backing frame is dropped due to
       error/timeout/finished with result?
       - Keep the frame's index in the `FrameElement`/`FrameBox`/whatever
       - Loop through array, do a `compare_exchange(u16::from(this_frame.index), u16::MAX).ok()`. An
         error means it's not our PDU, success means yay reset.

  2. For the first PDU pushed into the frame, store its index in the
     `FrameElement`/`FrameBox`/whatever. When parsing the frame back, we can match up based on that
     expected index.

     - Store command too for tighter matching?
     - No way to check if index is already in flight :(
       - Also no way to free them all on drop. I think solution 1. might have to be the way to go
       - Actually what about a tail counter? Eugh then I have to do overflowy maths and stuff. Maybe
         I cba.

# Multi-frame design

- Storage: Change from a list of PDUs to a list of Ethernet frames.

  - Slightly higher memory overhead (14 bytes more. Not that bad lol)
  - But a little more performant because the Ethernet header can be reused without writing to it all
    the time
    - `io_uring` can be zero copy!
  - A cleaner API around sending multiple PDUs per frame

- **Problem:** How do we free an Ethernet frame up for reuse if multiple PDUs are in it.

  - Maybe a **solution**: Keep a counter of PDUs that have been added to the frame, and when one is
    complete the counter is decremented? Then when it reaches zero we can drop the Ethernet frame.
    It's basically a `RefCell`!

- A trait to abstract over single sends and multiple sends? Would mean we don't need
  `send_receive`/`send_receive_deferred`, etc
- Scenarios:

  - Send a single command immediately (e.g. subdevice status, init commands, EEPROM, etc)
  - Send PDI that fits in a single frame
  - Send PDI that spans multiple frames
  - Send DC sync frame plus PDI that all fits in one frame
  - Send DC sync frame plus PDI that spans multiple frames

- Send single command
  - Claim frame
  - Write frame data
  - Async send/receive
  - Done
  - If single command doesn't fit in a single frame, bad times, tell the user to increase frame
    payload size or decrease data len. We won't try to cater for this case. Single command TX/RX is
    meant for tiny stuff like status checks, EEPROM reads, etc.
- Send multiple commands
  - If all commands don't fit in a single frame, error out. This is not something we want to cater
    to.
  - Use case is e.g. slave status and status code
  - Closure-based, returns a `try_zip` like I wrote the other day.
- Send PDI
  - One frame or spans multiple, doesn't matter
  - Get frame payload length
  - If we want to send DC sync PDU
    - Don't use closure-based API as we need to hold the DC response future along with pushing the
      first chunk fut to the receive vec/map. This means we can't just use the single returned
      future as they may complete at different times.
    - Reserve a frame
    - Enqueue DC sync PDU to reserved frame, store fut in Some(dc_sync_fut)
    - Split off PDI to min(pdi.len(), remaining frame payload length)
    - Write PDI chunk into frame and push returned future into vec/map
  - Chunk PDI into that length
  - For each chunk
    - Claim frame
    - Write PDI chunk into frame payload
    - Mark as sendable
    - Wake sender so frames are sent as fast as possible
    - Push frame future into a `heapless::Vec::<MAX_FRAMES>`, maybe with address and length so it
      can be matched up back into the PDI?
    - Read-back data is read into the beginning of the PDI, but we probably want to await _every_
      frame response to make sure it was all returned and nothing errored out.
  - `poll_fn`
    - If `take Some(dc_sync_fut)`, poll it and if it's ready, don't replace back, and set response
      to Some(dc_response).
      - If it's `None`, nothing to do.
    - If futures vec/map/queue is empty and `Some(dc_response)`, return `Poll::Ready(())`
    - Iterate through futures and poll each one
    - If it's ready, insert that result back into the passed-in `&mut pdi`
    - Remove future from vec (maybe it should be a map keyed by index or start address instead
      actually?)
  - PDI has been sent and returned completely, so return the response portion and optional DC
    response.

```rust

```

# Setup

## Windows

- Download npcap installer AND SDK from <https://npcap.com/#download>
  - Or install npcap with the Wireshark installer
- Copy npcap SDK to `./npcap-sdk-1.12`
- Make sure you install with "winpcap compatibility mode" option checked
- `$env:LIBPCAP_LIBDIR = 'C:\Windows\System32\Npcap'` - yes, the 32 bit version for some reason
- `$env:LIB = 'C:\Users\jamwa\Repositories\ethercrab\npcap-sdk-1.12\Lib\x64'` otherwise it fails to
  link
- `cargo run --example write-garbage-packet` or whatever

# Windows TwinCAT Wireshark capture (EK1100)

- Connect EK1100 through switch
- Connect USB ethernet to switch as well
- Tell TwinCAT to use USB ethernet
- Sniff traffic using motherboard NIC

## TwinCAT - finding topology

Double click device and go to `EtherCAT` tab in main view

## TwinCAT - see ports

Go to `Online` tab of any device

# SOEM on Windows

- Install VSCode `CMake Tools` extension
- Install `nmake` through `Build Tools for Visual Studio 2022` from
  [here](https://visualstudio.microsoft.com/downloads/)
  - I think it's the "C++ development tools" package or something
- Do this to the top level so the Windows `slaveinfo` builds:

  ```diff
    if(BUILD_TESTS)
      add_subdirectory(test/simple_ng)
  +   # add_subdirectory(test/linux/slaveinfo)
  -   add_subdirectory(test/linux/slaveinfo)
      add_subdirectory(test/linux/eepromtool)
  +   # add_subdirectory(test/linux/simple_test)
  -   add_subdirectory(test/linux/simple_test)
  +   add_subdirectory(test/win32/slaveinfo)
  +   add_subdirectory(test/win32/simple_test)
    endif()
  ```

- Copy and paste `CMakeLists.txt` from `test/linux/{test}/` to `test/win32/{test}/`.
- To squelch warnings as errors, find a line like the following, and remove `/WX`:

  ```cmake
  set(CMAKE_C_FLAGS "${CMAKE_C_FLAGS}  /WX")
  ```

- `nmake` only works in the "Visual Studio Powershell" or whatever, not vanilla powershell:
  - Open "Developer PowerShell for VS 2022" from the start menu
  - Follow instructions in SOEM readme:
    - `mkdir build`
    - `cd build`
    - `cmake .. -G "NMake Makefiles"`
    - `nmake`

# Windows and L2 sockets

<https://github.com/rustasync/team/issues/15>

Windows doesn't support L2 networking out of the box - a driver is required, e.g. libpcap or WinPcap

Because libpnet isn't really conducive to async, we pretty much can't do async on Windows at all.

Let's just use async stuff with an "in flight packets" queue for now

# Embassy async raw sockets

you can easily wrap them into async like this though

```rust
poll_fn(|cx| {
    if device.is_transmit_ready() {
        Poll::Ready(())
    } else {
        device.register_waker(cx.waker());
        Poll::Pending
    }
}).await;
device.transmit(pkt);
```

or for receive

```rust
let received_pkt = poll_fn(|cx| match device.receive() {
     Some(pkt) => Poll::Ready(pkt)
     None => {
          device.register_waker(cx.waker());
          Poll::Pending
    }
}).await;
```

Note however:

> ah, but receive() and register_waker() are separate calls, so an irq can definitely sneak in
> between them

So to fix:

```rust
async fn transmit(&mut self, pkt: PacketBuf) {
    poll_fn(|cx| {
        self.device.register_waker(cx.waker());
        match self.device.is_transmit_ready() {
            true => Poll::Ready(()),
            false => Poll::Pending,
        }
    }).await;
    self.device.transmit(pkt);
}

async fn receive(&mut self) -> PacketBuf {
    poll_fn(|cx| {
        self.device.register_waker(cx.waker());
        match self.device.receive() {
            Some(pkt) => Poll::Ready(pkt),
            false => Poll::Pending,
        }
    }).await
}
```

Can only register one waker for both TX + RX, so the suggestion is to have a device task and have
channels to send `PacketBuf`s between it and whatever else is waiting for them.

> PacketBuf is just ptr+length so it's fine to do a Channel<PacketBuf>, you won't be actually
> copying packets around

I can then have a single `poll_fn` that handles both TX and RX, using the channels appropriately.

Can't bind a task to an interrupt in Embassy, but I could do:

```rust
#[embassy::task]
async fn my_task(..) {
    loop {
        let pkt = device.receive().await;
        // process pkt
    }
}
```

But note:

> or a channel receive if you want to do tx/rx from separate tasks, because you can't share/split
> the Device itself

## ways to share stuff

- `put()` in a `Forever<Cell<Thing>>` in main, send `&'static Cell<Thing>` as an argument to tasks
- `put()` in a `Forever<RefCell<Thing>>` in main, send `&'static RefCell<Thing>` as an argument to
  tasks -- warning make sure not to hold a Ref across an await
- Global ThreadModeMutex, if all tasks run in thread mode, which is the default
- Global CriticalSectionMutex if not (if using InterruptExecutor or raw irq handlers)
- Channels kinda
- async Mutex (this is best for stuff you need to call async methods on, like shared spi/i2c buses)

Some rough pseudocode by @dirbaio on Element:

```rust
struct Client {
    state: RefCell<ClientState>,
}

struct ClientState {
    waker: WakerRegistration,
    requests: [Option<RequestState>; N],
}

struct RequestState {
    waker: WakerRegistration,
    state: RequestStateEnum,
}

enum RequestStateEnum {
    Created{payload: [u8;N]},
    Waiting,
    Done{response: [u8;N]},
}

#[embassy::task]
async fn client_task(device: Device, client: &'static Client) {
    poll_fn(|cx| {
        let client = &mut *self.client.borrow_mut();
        client.waker.register(cx.waker);
        device.register_waker(self.waker);

        // process tx
        for each request in client.requests{
            match request.state {
                Created(payload) => {
                    // if we can't send it now, try again later.
                    if !device.transmit_ready() {
                        break;
                    }
                    device.transmit(payload);
                    request.state = Waiting
                }
                _ => {}
            }
        }

        // process rx
        while let Some(pkt) = device.receive() {
            // parse pkt
            let req = // find waiting request matching the packet
            req.state = Done{payload};
            req.waker.wake();
        }
    })
}

impl Client {
    async fn do_request(&self, payload: [u8;N]) -> [u8;N] {
        // braces to ensure we don't hold the refcell across awaits!!
        let slot = {
            let client = &mut *self.client.borrow_mut();
            let slot: usize = // find empty slot in client.requests
            client.requests[slot] = Some(RequestState{
                state: Created(payload),
                ...
            });
            client.waker.wake();
            slot
        };

        poll_fn(|cx| {
            let client = &mut *self.client.borrow_mut();
            client.requests[slot].waker.register(cx.waker);
            match client.requests[slot].state {
                Done(payload) => Poll::Ready(payload),
                _ => Poll::Pending
            }
        }).await
    }
}
```

> the key is you share just data
>
> so instead of txing from the task doing the req, you just set some state saying "there's this
> request pending to be sent"
>
> and then the client_task sends it

# Cross platform logging

Support `defmt` as well as `log` (no_std and std) with
[this](https://github.com/embassy-rs/embassy/blob/master/embassy/src/fmt.rs)

It has feature flags and macros to select between them.

dirbaio
[says](https://matrix.to/#/!yhYQWkYvbEWASqoKIM:matrix.org/$R8BYMVarPIRBZpuqOtkINc6p6KF2jUBpmXX4UNITYUQ?via=beeper.com&via=matrix.org&via=grusbv.com):

> paste [this](https://github.com/embassy-rs/embassy/blob/master/embassy/src/fmt.rs) into your
> crate, it allows using either `defmt` or `log` directly depending on Cargo features
>
> so you use `log` directly, you don't need a `defmt`->`log` adapter
>
> `defmt` needs linker hax to compile for linux targets, and doesn't compile for windows, mac, ios
> targets at all

# EEPROM read abstraction

EEPROM is word-based, but I want to read bytes so let's make an abstraction:

- EEPROM section iterator
- Start address as u16
- Length limit
- next().await gives me a byte at a time
  - Read 4/8 byte chunk, split off the first element until error, then increment address by chunk
    len / 2 and read another chunk
- skip(n) takes number of BYTES to skip
  - next u16 read address = current byte address / 2
  - replace chunk
  - if n is odd, call next() once to discard initial byte
- take_vec()
  - while vec can be pushed AND address is less than start + len, next().await
  - return vec on push error
  - Can be optimised with chunked pushes:
    - Read chunk, increment byte counter by chunk len, address by chunk len /2
    - Check remaining bytes in section and clip chunk
    - If chunk len is 0, return buffer, else:
    - Write chunk into return buffer
      - If write fails,
        - If we have space left, push partial chunk
        - Return buffer

# PDI

- Send PDI in chunks of `MAX_PDU_DATA`
- Core concept for thread safety: Slaves must be grouped
  - I can provide a nice API to get a single default group maybe
- Each group has its own PDI with a start address and length or whatever
- Iteration looks like this:

  ```rust
  let interval = Interval::new(Duration::from_millis(2));

  while let Some(_) = interval.next().await {
      group.tx_rx(&client).await;

      // Or whatever
      group.slave_by_index(0).outputs()[0] = 0xff;

      for slave in group.slaves_mut() {
          // Whatever
      }
  }
  ```

- Because each group is non-overlapping, we don't need to lock in `group.tx_rx().await`
- Groups are `Send` but not `Sync`; they can be moved to a thread but NOT shared.
- Sharing data between groups is up to the user, e.g. SPSC or other means to keep things safe. A bit
  of a copout but it means we don't get data races by default.
- Assign a group ID to each discovered slave before init, using a closure and maybe make group IDs
  generic to the user. Or maybe not; a group ID should map to a number for indexing reasons. Maybe
  just `Into<usize>` for the user's type? That would allow nice enums like `Safety, Servos, IO`, etc
- Groups should be separate from `client` - the list of slaves should store enough info to be useful
  to users. `client` should only be used for tx/rx

## Another idea for slave config/grouping

```rust
struct Groups {
    io: SlaveGroup<16>,
    servos: SlaveGroup<3>
}

// Closure must return a reference to a SlaveGroup to insert the slave into
let groups = client.init::<Groups>(|&mut groups, slave, idx| {
  if slave.id == 0xwhatever {
    Some(&mut groups.io)
  } else if slave.manu == 0xwhatever {
    Some(&mut groups.servos)
  } else {
    // Might want to ignore a detected slave - maybe it's not a recognised device. Add config option
    // to make this an error?
    None
  }
}).await.unwrap();

// init()
// - Detect all slaves
// - TODO: Add some basic info to them like manufacturer ID, name, etc so people can identify them
//   - For now I'll just return an index
// - Loop through them all, passing into closure to get the group to insert into
// - While we have a ref to the group, initialise the slave in it and update its PDI map
// - Return groups when we're done

struct SlaveGroup<const N: usize> {
    slaves: heapless::Vec<Slave, N>
}

impl SlaveGroup {
    fn new() -> Self {
        // This could probably impl default
        Self { slaves: heapless::Vec::new() }
    }

    async fn init_from_eeprom_and_push(&mut self, &client, slave) -> Result<(), Error> {
        // What the client already does, but scoped to a group.
        // Needs to also return the PDI offset ready for the next group to use.
    }

    pub async fn tx_rx(&self, &client) -> Result<(), Error> {
        //
    }
}
```

# Group configuration without `Box dyn`

Forget hotplug and dynamic reconfig for now so we don't have to store a closure in each group. This
can be added back in when stable has the right features to make it not-box. Errors will either mean
looping back to the top of the program or just panicking I'm afraid...

- Still box future but works with new design
  <https://play.rust-lang.org/?version=stable&mode=debug&edition=2021&gist=ee24e35922409fdcf9bac6c490c2e373>
- No more box, but it's only `FnOnce` so doesn't work in a loop like EtherCrab needs
  <https://play.rust-lang.org/?version=stable&mode=debug&edition=2021&gist=8d28e2ae86c98bfeedc03d23d4790a4f>
- It works! We can even have `FnMut` if we want
  <https://play.rust-lang.org/?version=stable&mode=debug&edition=2021&gist=cb13fe888d80b2ab6daa0f91bfd92120>

Perhaps we can refactor the groups using typestates like e.g.

```rust
let groups = client.assign_slaves(Groups::default(), |g, s| { ... }).await?;

// Groups are in PreOp at this point
let Groups { fast, slow } = client.init(...);

// These could move into their respective tasks/threads if they don't use outside data, or make sure
// outside data is `Sync` or whatever.
let fast: SlaveGroup<Op> = fast.start_op(|slave| { ... }).await?;
let slow: SlaveGroup<Op> = slow.start_op(|slave| { ... }).await?;

impl SlaveGroup<PreOp> {
    // Individual phases can be opened up in the future, like `into_safe_op`, `into_op`, etc

    /// Put the group all the way to PreOp -> SafeOp -> Op
    pub async fn into_op(self, hook: F) -> Result<SlaveGroup<Op>>, ()> {
        // Do group stuff in PreOp

        // Call hook

        // Do more group stuff or whatever

        // Put group into SafeOp

        // Internal methods which we could make pub in the future
        let g = Self { ..., _state: GroupState::SafeOp }

        // Put group into Op, wait for state

        // All good
        Ok(Self { ..., _state: GroupState::Op })
    }
}

impl SlaveGroup<Op> {
    async fn tx_rx(&self) -> Result<(), Error> {
        // ...
    }

    // Future stuff. Neat!
    // Maybe INIT instead of PREOP?
    pub async fn shutdown(self) -> Result<SlaveGroup<PreOp>>, Error>
}
```

This requires that slaves can be put into different states as groups, not all at once, but I think
this is fine as EtherCrab does this already.

`SlaveGroup<SafeOp>` and `SlaveGroup<Op>`.

# Remote packet capture on Linux

Allow root login with `PermitRootLogin yes` in `/etc/ssh/sshd_config` (then restart sshd service).

```bash
ssh root@ethercrab tcpdump -U -s0 -i enp2s0 -w - | sudo wireshark -k -i -
```

# Jitter measurements

All run with `--release` on `Linux 5.15.0-1032-realtime #35-Ubuntu SMP PREEMPT_RT`, i3-7100T.

```bash
just linux-example-release jitter enp2s0
```

| Test case                                                      | standard deviation (ns) | mean (ns) | comments                                          |
| -------------------------------------------------------------- | ----------------------- | --------- | ------------------------------------------------- |
| 5ms `tokio` timer                                              | 361909                  | 4998604   |                                                   |
| 5ms `tokio-timerfd`                                            | 4264\*                  | 4997493   | std. dev. jumped from 2646 to 4508ns during test  |
| 5ms `tokio-timerfd` + pinned tx/rx thread                      | 14315\*                 | 4997157   | std. dev. jumped from 2945 to 15280ns during test |
| 5ms `tokio-timerfd` + pinned tx/rx thread + pinned loop thread | 117284                  | 4997443   | std. dev. jumps around a lot but max is huge      |
| 5ms `smol::Timer`                                              | 5391\*                  | 4997841   | std. dev. jumped to 11558ns during test           |
| 1ms `smol::Timer`                                              | 21218\*                 | 997581    | Highest std. dev. I saw was 21k ns                |
| 1ms `smol::Timer`, another run                                 | 19224\*                 | 997483    | Just under 2% jitter at 1ms                       |

## Results from oscilloscope

This is running the "1ms `smol::Timer`, another run" case from above, measuring output 1 of an
EL2004 hooked up to an EK1100.

| Measurement                  | Value         |
| ---------------------------- | ------------- |
| Mean (software)              | 997456ns      |
| std. dev. (software)         | 72038ns (~7%) |
| Period std. dev. (oscope)    | ~15us         |
| Frequency std. dev. (oscope) | ~3.5Hz        |

The display flickers occasionally showing some occasional glitches in the output stream which isn't
great. I'm not sure where those might be coming from...

## Kernel/OS/hardware tuning

A good looking guide at <https://rigtorp.se/low-latency-guide/>.

`tokio` is awful, `smol` is much better for jitter. Tokio has 17us SD jitter (oscope) or 347246ns
jitter (34.72) (SW).

`tokio-timerfd` makes things a little better at ~18-23% SW jitter or 11us SD jitter (oscope).

Single threaded `tokio-timerfd` is a little better still at ~10% SW jitter or 4us oscope jitter.

`smol` still seems better.

List threads, priority and policy with

```bash
ps -m -l -c $(pidof jitter)
```

### First changeset

- `tuned-adm profile latency-performance`
- Disable hyperthreading at runtime with `echo off > /sys/devices/system/cpu/smt/control`

#### Run 1

Promising results. 4500ns SD (software), saw a jump up to 16360ns. Seems happy at 15000ns (1.54%)
SD.

Oscope shows 600ns SD (but is shrinking all the time).

I think there was one hiccup at the beginning of the test which skewed the results.

#### Run 2

650ns SW SD, 20ns oscope SD, but reducing continuously.

Saw a jump up to 1156 SW SD not long after startup again, but reducing all the time during run.

Saw a 5676ns (SW) jump, pushed oscope SD up to ~300ns.

Saw a 9265ns (SW) jump, pushed oscope SD up to ~350ns.

Saw a 19565ns (2%) SW jump, pushed oscope SD up to 772ns.

Saw a 28264ns (2.8%) SW jump, pushed oscope SD up to 700ns.

### Second changeset

Setting thread priority in code.

#### Without thread prio:

```
❯ ps -m -l -c $(pidof jitter)
F S   UID     PID    PPID CLS PRI ADDR SZ WCHAN  TTY        TIME CMD
4 -  1000   25164   25158 -     - - 35337 -      pts/5      0:01 ./target/release/examples/jitter enp2s0
4 S  1000       -       - TS   19 -     - -      -          0:00 -
1 S  1000       -       - TS   19 -     - -      -          0:00 -
1 S  1000       -       - TS   19 -     - -      -          0:00 -
```

Started at ~5% SW jitter, shrank down to <1% after about 2 mins of runtime. Oscope shows 6ns SD BUT
jumped up to 1us from one enormous glitch. SW SD went to 1.21%.

#### With this thread prio code:

```rust
let thread_id = thread_native_id();
assert!(set_thread_priority_and_policy(
    thread_id,
    ThreadPriority::Crossplatform(ThreadPriorityValue::try_from(99u8).unwrap()),
    ThreadSchedulePolicy::Realtime(RealtimeThreadSchedulePolicy::Fifo)
)
.is_ok());
```

This gives:

```
❯ ps -m -l -c $(pidof jitter)
F S   UID     PID    PPID CLS PRI ADDR SZ WCHAN  TTY        TIME CMD
4 -  1000   23969   23904 -     - - 35340 -      pts/5      0:01 ./target/release/examples/jitter enp2s0
4 S  1000       -       - FF  139 -     - -      -          0:01 -
1 S  1000       -       - FF  139 -     - -      -          0:00 -
1 S  1000       -       - FF  139 -     - -      -          0:00 -
```

##### Run 1

Max 0.06% SW SD jitter (561ns). Oscope SD showing ~6ns SD jitter. Wow!

After a few minutes of running it's a little worse with max 0.99% SW SD (10000ns) but still not bad.

##### Run 2

1800s duration (went to the shops lol)

```
1800 s: mean 0.998 ms, std dev 0.010 ms (0.99 % / 2.18 % max)
```

Pretty darn good. To summarise, this is with:

- i3-7100T / 8GB DDR4 (Dell Optiplex 3050 micro)
- Realtek somethingsomething gigabit NIC
- EK1100 + EL2004 slave device
- `smol`
- 4 total threads
  - ```
    ❯ ps -m -l -c $(pidof jitter)
    F S   UID     PID    PPID CLS PRI ADDR SZ WCHAN  TTY        TIME CMD
    4 -  1000   23969   23904 -     - - 35340 -      pts/5      0:01 ./target/release/examples/jitter enp2s0
    4 S  1000       -       - FF  139 -     - -      -          0:01 -
    1 S  1000       -       - FF  139 -     - -      -          0:00 -
    1 S  1000       -       - FF  139 -     - -      -          0:00 -
    ```
- `tuned-adm profile latency-performance`
- Hyperthreading disabled with `echo off > /sys/devices/system/cpu/smt/control` (2 cores on
  i3-7100T)
- 1ms cycle time
- Setting thread priority to 99 and `FIFO` policy.

# Distributed clocks (again)

- Decent resource here:
  <https://infosys.beckhoff.com/english.php?content=../content/1033/ethercatsystem/2469118347.html&id=>
- Sync modes:
  <https://infosys.beckhoff.com/english.php?content=../content/1033/ethercatsystem/2469122443.html&id=>

# A different way of sending commands

The target here is reducing code size hopefully with less monomorphisation when sending/receiving
slices. We'll see.

```rust
enum Writes {
    Brw,
    Lrw,
}

enum Reads {
    Brd,
    Lrd,
}

enum Command {
    Read(Reads),
    Write(Writes),
}

impl Command {
    pub fn brd() -> Self {
        Self::Read(Reads::Brd)
    }

    // etc...
}

impl Writes {
    /// Send data and ignore the response
    pub fn send<T>(client: u8, value: T) -> Result<(), Error> {
        // ...
    }

    /// Send a slice and ignore the response
    pub fn send_slice(client: u8, value: &[u8]) -> Result<(), Error> {
        // ...
    }

    pub fn send_receive<T>(client: u8, value: T) -> Result<T, Error> {
        // ...
    }

    pub fn send_receive_slice<T>(client: u8, value: &[u8]) -> Result<RxFrameDataBuf, Error> {
        // ...
    }
}

impl Reads {
    pub fn receive<T>(client: u8) -> Result<T, Error> {
        // ...
    }

    pub fn receive_slice(client: u8, len: u16) -> Result<RxFrameDataBuf, Error> {
        // ...
    }
}
```

# Profiling

To profile an example:

## Linux

```bash
# Ubuntu
apt install linux-tools-common linux-tools-generic linux-tools-`uname -r`
# Debian
sudo apt install linux-perf

RUSTFLAGS="-C force-frame-pointers=yes" cargo build --example <example name> --profile profiling

# <https://stackoverflow.com/a/36263349>
# To get kernel symbols in the perf output
echo 0 | sudo tee /proc/sys/kernel/kptr_restrict
# OR
sudo sysctl -w kernel.kptr_restrict=0

# Read current value
sudo sysctl kernel.kptr_restrict

# This should show non-zero addresses now. Will show zeros without sudo.
sudo cat /proc/kallsyms

# Might need sudo sysctl kernel.perf_event_paranoid=-1
# Might need sudo sysctl kernel.perf_event_mlock_kb=2048
sudo setcap cap_net_raw=pe ./target/profiling/examples/<example name>
sudo perf record --call-graph=dwarf -g ./target/profiling/examples/<example name> <example args>

# To record benchmarks
sudo perf record --call-graph=dwarf -g -o bench.data ./target/release/deps/pdu_loop-597b19205907e408 --baseline master --bench

# Ctrl + C when you're done

# Must use sudo to get kernel symbols
sudo perf report -i perf.data

# This won't show kernel symbols (possibly only when over SSH?)
sudo chown $USER perf.data
samply load perf.data

# Forward the port on a remote machine with
ssh -L 3000:localhost:3000 ethercrab
# Otherwise symbols aren't loaded
```

## macOS

- Install XCode
- [`sudo xcode-select -switch /Library/Developer/CommandLineTools`](https://apple.stackexchange.com/a/446563)
- `cargo instruments --template 'CPU Profiler' --profile profiling --bench pdu_loop -- --bench --profile-time 5`

Other templates can be listed with `xctrace list templates`
