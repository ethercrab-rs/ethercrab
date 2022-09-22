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
      add_subdirectory(test/linux/simple_test)
  +   add_subdirectory(test/win32/slaveinfo)
    endif()
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
