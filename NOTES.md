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
sudo perf record --call-graph=dwarf -g -o bench.data ./target/release/deps/pdu_loop-597b19205907e408 --bench --profile-time 5 'bench filter here'

# Ctrl + C when you're done

# Must use sudo to get kernel symbols
sudo perf report -i perf.data

# This won't show kernel symbols (possibly only when over SSH?)
sudo chown $USER perf.data
samply import perf.data

# Forward the port on a remote machine with
ssh -L 3000:localhost:3000 ethercrab
# Otherwise symbols aren't loaded
```

## macOS

- Install XCode
- [`sudo xcode-select -switch /Library/Developer/CommandLineTools`](https://apple.stackexchange.com/a/446563)
- `cargo instruments --template 'CPU Profiler' --profile profiling --bench pdu_loop -- --bench --profile-time 5`

Other templates can be listed with `xctrace list templates`

# Distributed clocks investigation

- <https://infosys.beckhoff.com/english.php?content=../content/1033/ethercatsystem/2469122443.html#2470228491&id>
- <https://wiki.bu.ost.ch/infoportal/embedded_systems/ethercat/understanding_ethercat/understanding_sync_with_dc>
- The Reference Clock is the first DC-supporting subdevice, and is referred to as the _system time_.

## Setup

- Two LAN9252s
- Oscilloscope attached to `IRQ` pin (also has `LATCH0` and `LATCH1` broken out).
- `RUST_LOG=info,ethercrab::dc=debug just linux-example-release dc enp2s0`

## Baseline

Running `RUST_LOG=info,ethercrab::dc=debug just linux-example-release dc enp2s0` shows a measured
delay of 720ns between slaves. The oscilloscope shows a mean of 725ns, min/max 705/745ns, std dev
11.6ns so EtherCrab's measured value seems to work well.

`LATCH0` and `LATCH1` give no outputs as the `dc` example hasn't configured anything yet. I think...

## Master sync

- A decent explanation: <https://www.acontis.com/en/dcm.html> discusses "DCM" (Distributed Clocks
  Master Synchronization)
- First DC slave can be used as reference, and the master syncs FROM it. Also the option to sync the
  master _to_ the first slave.

## SOEM `sync01()`

- IRQ pins show network propagation delay as expected - around 700ns. This is even with sync0
  enabled for the first slave.

## Sync time

- <https://download-edge.beckhoff.com/download/document/automation/twincat3/TF6225_TC3_EtherCAT_External_Sync_EN.pdf>

  Shows roughly 30 second sync time in section 6.1.3.

- Sending system time in static sync instead of 0 helps quite a lot

## Using the MainDevice clock as the network reference is not a thing

- Even with [DCM](https://www.acontis.com/en/dcm.html), this is just syncing with the first
  DC-supporting MainDevice in the network.

## Reset

- <https://download.beckhoff.com/download/document/io/ethercat-development-products/ethercat_esc_datasheet_sec1_technology_2i3.pdf>
  9.1.4

  > Before starting drift compensation, the internal filters of the Time Control Loop must be reset.
  > Their current status is typically unknown, and they can have negative impact on the settling
  > time. The filters are reset by writing the Speed Counter Start value to the Speed Counter Start
  > register (0x0930:0x0931). Writing the current value of the register again is sufficient to reset
  > the filters

## Showing dropped packets on Linux

- `ip -s link show enp2s0`
  <https://www.cyberciti.biz/faq/linux-show-dropped-packets-per-interface-command/>
- Or `netstat -i`
- Or `ethtool -S enp2s0`
  <https://www.thegeekdiary.com/troubleshooting-slow-network-communication-or-connection-timeouts-in-linux/>
- Or `cat /sys/class/net/enp2s0/statistics/rx_dropped` <https://serverfault.com/a/561132>
- Setting:

  ```
  net.core.rmem_max = 12500000
  net.core.wmem_max = 12500000
  ```

  greatly increases the number of `rx_errors` and `align_errors`.

- Doesn't seem to be dependent on switching threads or executors. `smol::spawn`, `tokio::spawn` and
  `smol::block_on` in a thread pinned to core 0 make no difference.
- Wireshark shows the last packet not being received so I don't think it's my weird impl.
- The lost packet appears eventually!

  Logged with

  ```bash
  tshark --interface enp2s0 -f 'ether proto 0x88a4' -T fields -e frame.number -e frame.time_relative -e frame.time_delta -e eth.src -e frame.len -e ecat.idx -e ecat.cmd -e ecat.adp -e ecat.ado
  ```

  Nearly a minute later:

  ```
  212171  229.185843925   0.001081760     10:10:10:10:10:10       0x06    0x0e    0x1000  0x0910
  212172  229.185851399   0.000007474     10:10:10:10:10:10       0x07    0x0c
  212173  229.185881114   0.000029715     12:10:10:10:10:10       0x06    0x0e    0x1000  0x0910
  212174  283.980272341   54.794391227    12:10:10:10:10:10       0x07    0x0c
  ```

  Another less drastic example:

  ```
  286913  792.417020063   0.001086499     10:10:10:10:10:10       0x19    0x0e    0x1000  0x0910
  286914  792.417028279   0.000008216     10:10:10:10:10:10       0x1a    0x0c
  286915  792.417051592   0.000023313     12:10:10:10:10:10       0x19    0x0e    0x1000  0x0910
  286916  795.080750017   2.663698425     12:10:10:10:10:10       0x1a    0x0c
  ```

  12 seconds this time

  ```
  50367   15.443982363    0.002090013     10:10:10:10:10:10       0x00    0x0c
  50368   15.443990028    0.000007665     10:10:10:10:10:10       0x1f    0x0e    0x1000  0x0910
  50369   15.444012700    0.000022672     12:10:10:10:10:10       0x00    0x0c
  50370   15.444020715    0.000008015     12:10:10:10:10:10       0x1f    0x0e    0x1000  0x0910
  50371   15.446105098    0.002084383     10:10:10:10:10:10       0x01    0x0e    0x1000  0x0910
  50372   15.446112552    0.000007454     10:10:10:10:10:10       0x02    0x0c
  50373   15.446140353    0.000027801     12:10:10:10:10:10       0x01    0x0e    0x1000  0x0910
  50374   27.842931439    12.396791086    12:10:10:10:10:10       0x02    0x0c
  ```

  ```
  1705    0.852359273     0.002079843     10:10:10:10:10:10       36      0x09    0x0e    0x1000  0x0910
  1706    0.852366577     0.000007304     10:10:10:10:10:10       28      0x0a    0x0c
  1707    0.852395721     0.000029144     12:10:10:10:10:10       60      0x09    0x0e    0x1000  0x0910
  1708    58.197952942    57.345557221    12:10:10:10:10:10       60      0x0a    0x0c
  ```

  New record: 2 minutes!

  ```
  39151   9.952622248     0.001104738     10:10:10:10:10:10       36      0x17    0x0e    0x1000  0x0910
  39152   9.952626089     0.000003841     10:10:10:10:10:10       28      0x18    0x0c
  39153   9.952637403     0.000011314     12:10:10:10:10:10       60      0x17    0x0e    0x1000  0x0910
  39154   130.431369768   120.478732365   12:10:10:10:10:10       60      0x18    0x0c
  ```

  `cat /sys/class/net/enp2s0/statistics/rx_dropped` hasn't changed between running these tests. I
  guess because it wasn't really dropped huh...

  **Works fine on i210 in `moreports`**.

- Allow receive of all packets, bad FCS or not: `sudo ethtool -K eth0 rx-fcs on rx-all on`
  <https://stackoverflow.com/a/24175679/383609>. This allows Wireshark to capture all packets.
  Packet size goes up by 4 bytes (32 bit CRC at end).

  **Checksums seem fine; nothing changes if I do `sudo ethtool -K eth0 rx-fcs off rx-all on`.**

- Kernel upgrade and a switch to Debian made no difference

  Before:

  ```
  ❯ sudo ethtool -i enp2s0
  driver: r8169
  version: 5.15.0-1032-realtime
  firmware-version: rtl8168h-2_0.0.2 02/26/15
  expansion-rom-version:
  bus-info: 0000:02:00.0
  supports-statistics: yes
  supports-test: no
  supports-eeprom-access: no
  supports-register-dump: yes
  supports-priv-flags: no
  ```

  After:

  ```
  ❯ sudo ethtool -i enp2s0
  driver: r8169
  version: 6.1.0-18-rt-amd64
  firmware-version: rtl8168h-2_0.0.2 02/26/15
  expansion-rom-version:
  bus-info: 0000:02:00.0
  supports-statistics: yes
  supports-test: no
  supports-eeprom-access: no
  supports-register-dump: yes
  supports-priv-flags: no
  ```

## DC first pulse investigation

- Two LAN9252
- Oscilloscope to see relative SubDevice 1/2 SYNC0 pulses
- Sipeed logic analyser to capture first pulse

Initial results show that something is not right - the second SubDevice SYNC0 starts at 950ms,
whereas the first SubDevice starts at 650ms. These values are due to 5% pre-trigger in PulseView.

Second and third runs happens at 945ms. This doesn't correlate with power-on time of the second
SubDevice, so it's not relative to that.

`950 - 650 = 300` though, and the device times (in a subsequent run, so subject to a bit of clock
drift) are

- 2110377260481
- 2110693458806

With a delta of 316198325ns, or 316.1983ms. This is probably where the discrepancy is coming from.

- **Theory:** We need to let the clocks synchronise and start sending FRMWs before we can set the
  first SYNC0 start time.

  **Result:** No change when moving the first pulse calculation after the clock sync.

Something I did notice though is the first pulse of the second SubDevice is 26us before the nearest
SYNC0 from the first SubDevice, which may just be clock drift, meaning we are actually aligned, just
300ms later.

- **Theory:** While we set up the first sync pulse, the FRMW isn't being sent, allowing the clocks
  to drift. So, if we run the TX/RX concurrently with the setup, maybe the pulses will at least
  align, even if the second SubDevice still starts 300ms later.

  **Result:** Yes, the first sync pulse of the second SD is much closer to the nearest first if we
  do `Command::frmw(0x1000, RegisterAddress::DcSystemTime.into())` with `futures_lite::future::or`,
  so this checks out.

Logging the time between each SYNC0 init, it turns out that configuring each slave takes 316ms,
exactly the amount of time the second SubDevice's first SYNC0 pulse is delayed by. That explains the
delay which is nice.

The next question is to figure out when the process data loop should wait until, based on that first
delay.

I think the code as currently written does a modulo on the cycle time, so calculates the start time
based on the DC System Time rounded to a multiple of the cycle time. This should mean we can just do
modulo everywhere to figure out the offset in the cycle. This holds AS LONG AS the SYNC1 cycle time
is zero. I won't bother supporting SYNC1 for now.

With a cycle shift of 0, the IRQ pin of the LAN9252 _seems_ to correlate with the value of
`dc_time % sync0_cycle_time`.

Trying to align the master cycle to exactly the start of the next cycle will mean the PI controller
will never stabilise, as there's always the transmission delay.

I'm getting 1ms std-dev on my oscope for a 5ms cycle on `ethercrab`.

Messing around with the PI parameters makes a huge difference to stability.

### Jitter improvements

- Using `pollster::block_on` makes no difference to jitter over just using `smol`. I'm seeing +/-1ms
  of jitter on a 5ms cycle. Not good enough. `cassette::block_on` makes no difference either, so
  it's either in the PI loop or the timer.
- Commenting out the stats gathering/printing/Ctrl+C hook also makes no difference to the jitter.
- `sudo tuned-adm profile latency-performance`, `realtime`, `throughput-performance` makes no
  difference to jitter.

- Setting main thread prio helps quite a bit

  ```rust
  thread_priority::set_current_thread_priority(ThreadPriority::Crossplatform(
      ThreadPriorityValue::try_from(48u8).unwrap(),
  ))
  .expect("Main thread prio");
  ```

  Using `90` doesn't make any difference and requires root, so 48 is fine.

  The IRQ converges on SYNC0, but sometimes gets disturbed and takes a while to settle again.

- Setting core affinity in code doesn't change anything.
- Disabling HT with `noht` GRUB option doesn't work on `ethercrab` due to AMD-ness
- Setting performance governor from `conservative` to `performance` doesn't help
- Setting `processor.max_cstate=0` in grub makes no difference
- Changing the `smol::block_on` that wraps the `dc` example to `pollster::block_on` or
  `cassette::block_on` makes no difference.
- Putting the PD loop in a FIFO/48 RT thread doesn't help
- Clock tick tuning like [here](https://docs.kernel.org/timers/no_hz.html) setting GRUB options
  `idle=mwait processor.max_cstate=0 intel_idle.max_cstate=0` sort of helps. Now down to +/-500us of
  jitter.
- Somewhat mercifully, using io_uring over `smol` does not actually help the jitter
- `moreports` exhibits the same ~500us jitter. Maybe slightly more, but not significant.
- A very quick test with `spin_sleep` makes things _even worse_.
- Feeding an EMA into the PI loop makes things worse
- **Solution found: Get rid of the PI loop and just naively calculate the next delay. ARGH!**

  Thanks to Valentin for poking me to actually try this :)

  This gives about 50us of jitter on `ethercrab`.

### Modulo and rounding errors

If the shift time is very close to the start or end of the cycle, it skips cycles. This is bad, so
let's fix it.

## Plotting `dc-pd.csv`

```gnuplot
set datafile separator ','
# set xdata time # tells gnuplot the x axis is time data
# set ylabel "First Y " # label for the Y axis
set autoscale fix
set key top right outside autotitle columnhead
set xlabel 'ECAT time (ns)' # label for the X axis
set format x "%.0f"
set format y "%.0f"

set ylabel "Value"

plot './dc-pd.csv' using 1:2 title "Data" with lines # , '' using 1:3 title "PI out" with lines axis x1y2
```

## Wtf is `AssignActivate` in EtherCAT ESI?

Finally,
[some information](https://github.com/OpenEtherCATsociety/SOEM/issues/482#issuecomment-782878892)!:

> You are almost there. In the ESI file the AssignActivate value is 0x0700. This means 0x00 has to
> be written to ESC register 0x0980 and 0x07 to ESC register 0x0981. This will configure both SYNC0
> and SYNC1. You only activate SYNC0.

Uh but [also](https://github.com/OpenEtherCATsociety/SOEM/issues/635#issuecomment-1237534462):

> Just if anyone is following this thread. Some drivers require to setup a special address to
> activate Sync0 as the latching time, regardless of ec_dSync0 instuction.
>
> My particular driver is set up by calling
>
> `uint16 activate = 1; ec_SDOwrite(1, 0x0300, 0x00, true, sizeof(activate), &activate, EC_TIMEOUTRXM);`
>
> Most of the time, the object required to command Sync0 is specified on the xml file in
> OpMode/Assignactivate: `<AssignActivate>#x0300</AssignActivate>`

Note
`int ec_SDOwrite(uint16 Slave, uint16 Index, uint8 SubIndex, boolean CA, int psize, const void *p, int Timeout)`

**Yeah I figured it out:** `AssignActivate` is the bitmask for register 0x0981
`RegisterAddress::DcSyncActive`. `0x0700` turns on SYNC1 AND SYNC0. `0x0300` turns on `SYNC0` only
(both along with the DC sync enable bit).

# Ultimate Linux networking guide

<https://ntk148v.github.io/posts/linux-network-performance-ultimate-guide>
# XDP stuff

- List of supporting drivers:
  <https://github.com/iovisor/bcc/blob/master/docs/kernel-versions.md#xdp> as well as
  <https://github.com/xdp-project/xdp-project/blob/master/areas/drivers/README.org#xdp-driver-support-status>
- Intel docs on offload: <https://eci.intel.com/docs/3.0.2/development/tsnrefsw/bpf-xdp.html>. Even
  mentions EtherCAT
  [here](https://eci.intel.com/docs/3.0.2/development/tsnrefsw/bpf-xdp.html#linux-express-data-path-xdp)
  which is cool
- See what and where system libs are: `sudo ldconfig -p | rg bpf`
- Bidir AF_XDP explanation <https://hpnpl.net/posts/recapituatling-af-xdp/> (Medium backup
  <https://medium.com/high-performance-network-programming/recapitulating-af-xdp-ef6c1ebead8>)
- Busy polling seems to be good for running driver and app on same core?
  <https://github.com/xdp-project/bpf-examples/tree/5343ed3377471c7b7ef2237526c8bdc0f00a0cef/AF_XDP-example#busy-poll-mode>
- `libbpf` can (does?) load a default program if one is not provided
- `xsk` functions are provided by `libxdp`, which builds on top of `libbpf`. Mentioned
  [here](https://www.mankier.com/3/libxdp#Using_AF_XDP_sockets).
- `afxdp-rs` uses XDP stuff from `libbpf-sys` 0.7.0, however this is pretty out of date
- [`xsk-rs`](https://github.com/DouglasGray/xsk-rs) switched to `libxdp` a while back in
  <https://github.com/DouglasGray/xsk-rs/issues/21>
- Link errors when compiling with system-provided `libbpf-dev` _might_ be caused by the fact that
  it's ancient. Only some functions can't be found which might've been added in newer versions.
  Latest on Github is 1.4.2 at time of writing. Repo version is 0.5.0:

  ```
  ❯ apt-cache madison libbpf-dev
  libbpf-dev | 1:0.5.0-1ubuntu22.04.1 | http://archive.ubuntu.com/ubuntu jammy-updates/main amd64 Packages
  libbpf-dev | 1:0.5.0-1ubuntu22.04.1 | http://security.ubuntu.com/ubuntu jammy-security/main amd64 Packages
  libbpf-dev |  1:0.5.0-1 | http://archive.ubuntu.com/ubuntu jammy/main amd64 Packages
  ```

- Polling with a timeout of zero makes `libc::poll` return instantly
