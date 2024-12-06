# Performance tricks for Windows

These are very rough notes made by me (@jamwaffles), so apologies if the details are sparse.

TL;DR please try to use Linux. Windows is terrible for realtime even after making the tweaks below.
I think stuff like TwinCAT gets away with it because they use custom NIC drivers. There's an XDP for
Windows project which I'd love to get working which might help EtherCrab in the same way without
having to write custom drivers.

## Analysing your changes

Take a Wireshark capture of your existing application **before making any changes below**. I
recommend capturing at least 2 minutes of process data cycle, and capturing 3 runs for Scientific
Reasons.

It's worth capturing a few new traces for every change made to see what effect (if any) there was.
Up to you though.

Then, use [`dump-analyser`](https://github.com/ethercrab-rs/dump-analyser) to visualise Wireshark
traces.

- "Packet round trip times" should be as low as possible. Linux gets ~30us easily here. Windows...
  does not. The packet RTT is allowed to be quite jittery, however the RTT PLUS any computation you
  do in your process data cycle must not exceed the desired cycle time.
- "Cycle-cycle delta" records the time between two EtherCAT frames being sent from the MainDevice.
  It can reveal timing issues on the MainDevice, long calculation times in the process data cycle,
  etc. During OP, the cycle-cycle delta should be as close to your desired cycle time as possible.

## Windows realtime tuning

Follow the suggestions at
<https://learn.microsoft.com/en-us/windows/iot/iot-enterprise/soft-real-time/soft-real-time-device>,
especially the section on
[isolating cores](https://learn.microsoft.com/en-us/windows/iot/iot-enterprise/soft-real-time/soft-real-time-device#use-mdm-bridge-wmi-provider-to-configure-the-windowsiot-csp).
I didn't see a noticable difference on my test system when applying the other changes, but they
surely can't hurt.

After doing the `Set-CimInstance` stuff you should see `n` cores which aren't accessed by the
scheduler in task manager. E.g. if you run a Rust compile, all cores except the last `n` will be
utilised.

## NIC tweaks

Find your NIC in Device Manager and change:

- "Energy Efficient Ethernet" -> "Off"
- "Interrupt Moderation" -> "Disabled"
- "Interrupt Moderation Rate" -> "Off"
- "Receive Buffers" / "Transmit Buffers" -> 2048
- "Power Management" tab -> Disable all
- "Enable PME" -> "Off"
- "Flow Control" -> "Disabled"
- "Ultra Low Power Mode" -> "Disabled"

These are for an Intel i219 NIC, YMMV of course.

## NIC interrupt core pinning

We need to pin the NIC IRQ to the **same core as the one running the EtherCrab TX/RX task** (see
[here](#core-pinning)). This can be done with the
[Microsoft Interrupt Affinity Tool](https://www.techpowerup.com/download/microsoft-interrupt-affinity-tool/).
Run as administrator, find your NIC and click "set mask".

## EtherCrab tweaks

The
[Windows example](https://github.com/ethercrab-rs/ethercrab/blob/fe55c9e1ba1a9d189ccab6ad234e086890950202/examples/windows.rs)
is a good reference.

### Use the blocking TX/RX task

On Windows, `tx_rx_task` is deprecated since 0.5.1 as it has terrible performance.

Instead, use `tx_rx_task_blocking` in a separate thread, e.g.
[like this](https://github.com/ethercrab-rs/ethercrab/blob/master/examples/windows.rs#L98-L99).
Performance is much improved over `tx_rx_task`.

> You can set `spinloop: true` for... maybe some performance improvement? However this will peg one
> CPU core to 100% for marginal gains.

### Core pinning

EtherCrab requires at least two tasks; the main task/thread and a TX/RX thread. Pin these tasks to
two separate cores, ideally the ones you isolated in
[Windows realtime tuning](#windows-realtime-tuning).

Core pinning can be done like
[this](https://github.com/ethercrab-rs/ethercrab/blob/6d9fa85047cde6f20bfb5f1499daa9eb033e70fe/examples/windows.rs)
(main thread) and like
[this](https://github.com/ethercrab-rs/ethercrab/blob/6d9fa85047cde6f20bfb5f1499daa9eb033e70fe/examples/windows.rs#L94-L96)
for the TX/RX thread using the [`core-affinity`](https://docs.rs/core_affinity) crate:

```rust
let core_ids = core_affinity::get_core_ids().expect("Get core IDs");

// Pick the non-HT cores on my Intel i5-8500T test system. YMMV!
let main_thread_core = core_ids[0];
let tx_rx_core = core_ids[2];

// Pinning this and the TX/RX thread reduce packet RTT spikes significantly
core_affinity::set_for_current(main_thread_core);

thread_priority::ThreadBuilder::default()
    .name("tx-rx-thread")
    // For best performance, this MUST be set if pinning NIC IRQs to the same core
    .priority(ThreadPriority::Os(ThreadPriorityOsValue::from(
        WinAPIThreadPriority::TimeCritical,
    )))
    .spawn(move |_| {
        // VVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVV
        core_affinity::set_for_current(tx_rx_core)
            .then_some(())
            .expect("Set TX/RX thread core");
        // ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

        tx_rx_task_blocking(&interface, tx, rx, TxRxTaskConfig { spinloop: false })
            .expect("TX/RX task");
    })
    .unwrap();
```

### Thread priority

Set the TX/RX thread to high priority using something like the
[`thread-priority`](https://docs.rs/thread-priority) crate, like
[this](https://github.com/ethercrab-rs/ethercrab/blob/6d9fa85047cde6f20bfb5f1499daa9eb033e70fe/examples/windows.rs#L94-L96):

```rust
thread_priority::ThreadBuilder::default()
    .name("tx-rx-thread")
    // For best performance, this MUST be set if pinning NIC IRQs to the same core
    .priority(ThreadPriority::Os(ThreadPriorityOsValue::from(
        WinAPIThreadPriority::TimeCritical,
    )))
```

### Tweak EtherCrab timeouts

EtherCrab has a few places where it will repeatedly poll a SubDevice for status, data, etc. A small
delay is introduced in each loop to not DOS the network, however this uses Windows' ~15ms timers
which causes timeout issues all over the place. To fix, disable this functionality with:

```rust
Timeouts {
    // Windows timers are rubbish (min delay is ~15ms) which will cause a bunch of timeouts
    // if `wait_loop_delay` is anything above 0.
    wait_loop_delay: Duration::ZERO,
    // Other timeouts can be left alone, or increased if other issues are found.
    ..Default::default()
},
```

### Don't use the easy timers for process data cycles

Windows timers by default have about 15ms resolution which is atrocious. Rust (`std`, `tokio`,
`smol`, etc) use these timers by default.

Instead, one solution is to use [`spin-sleep`](https://docs.rs/spin_sleep) along with
[`quanta`](https://docs.rs/quanta) which can use the high resolution timers in Windows.

Setup is
[here](https://github.com/ethercrab-rs/ethercrab/blob/6d9fa85047cde6f20bfb5f1499daa9eb033e70fe/examples/windows.rs#L75-L82),
usage in the main process data cycle is
[here](https://github.com/ethercrab-rs/ethercrab/blob/6d9fa85047cde6f20bfb5f1499daa9eb033e70fe/examples/windows.rs#L150-L173):

```rust
// Both `smol` and `tokio` use Windows' coarse timer, which has a resolution of at least
// 15ms. This isn't useful for decent cycle times, so we use a more accurate clock from
// `quanta` and a spin sleeper to get better timing accuracy.
let sleeper = SpinSleeper::default().with_spin_strategy(SpinStrategy::SpinLoopHint);

// NOTE: This takes ~200ms to return, so it must be called before any proper EtherCAT stuff
// happens.
let clock = quanta::Clock::new();

//
// Do your init, into OP, etc here
//

let cycle_time = Duration::from_millis(5);

loop {
    let now = clock.now();

    group.tx_rx(&maindevice).await.expect("TX/RX");

    //
    // Application logic here
    //

    let wait = cycle_time.saturating_sub(now.elapsed());
    sleeper.sleep(wait);
}
```

### Increase your cycle time

This will mitigate jitter, but large hangs will still affect things.

### Increase watchdog timeouts

Sometimes large hangs can occur in timers and/or network traffic, causing SubDevice watchdog
timeouts. You can try increasing them with something like:

```rust
let watchdog_timeout_millis = 200u16;

subdevice
    .register_write(
        RegisterAddress::WatchdogDivider,
        // Multiplier for default 100us watchdog divider
        watchdog_timeout_millis * 10,
    )
    .await?;
```

### Enable distributed clocks

This will improve IO jitter from the device side, however many DC cycles will be missed on a very
jittery MainDevice.
