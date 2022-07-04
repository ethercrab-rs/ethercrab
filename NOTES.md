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

- `nmake` only works in the "Visual Studio Powershell" or whatever, not vanilla powershell.

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
