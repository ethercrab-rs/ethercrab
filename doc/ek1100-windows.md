# Running the `ek1100-windows` example

A Windows-focused version of the cross-platform [`ek1100`](../examples/ek1100.rs) example
([`examples/ek1100-windows.rs`](../examples/ek1100-windows.rs)). It brings up a Beckhoff
**EK1100** (or **EK1501**) coupler plus any modules behind it, takes the network to `OP`, and
then cycles process data, incrementing every output byte once per cycle.

## Why a separate Windows example?

The differences from the cross-platform [`ek1100`](../examples/ek1100.rs) example all come from
Windows limitations:

- **Blocking TX/RX on a dedicated OS thread.** There is no async packet backend on Windows, so
  the example runs [`tx_rx_task_blocking`](https://docs.rs/ethercrab/latest/ethercrab/std/fn.tx_rx_task_blocking.html)
  (Npcap-backed) on its own `std::thread`. On Windows the async `tx_rx_task` is deprecated and
  slow.
- **`spin_sleep` + `quanta` for cycle timing.** Windows' default timer resolution is ~15 ms,
  which is useless for a 5 ms cycle. A spin sleeper plus a `quanta` clock give a usable cycle.
- **`Timeouts { wait_loop_delay: Duration::ZERO, .. }`.** Any non-zero wait-loop delay is rounded
  up to the coarse timer granularity and produces spurious mailbox / state-transition timeouts.

The example already pins its threads and raises the TX/RX thread priority. For tuning beyond
that (NIC settings, IRQ affinity, MMCSS), see [`windows-tuning.md`](./windows-tuning.md).

## Prerequisites

You need **two** separate Npcap pieces: the SDK (to build) and the runtime (to run).

### 1. Npcap SDK — required to build

The `pcap` crate links against `wpcap.lib` from the **Npcap SDK**.

1. Download the **Npcap SDK** (the `.zip`, *not* the installer) from <https://npcap.com/#download>.
2. Extract it anywhere, e.g. `C:\Npcap-SDK`, so that `C:\Npcap-SDK\Lib\x64\wpcap.lib` exists.
   Use `Lib\x64` for 64-bit builds (the usual case), `Lib\ARM64` on ARM64, `Lib` for 32-bit.
3. Point the build at that directory with the `LIBPCAP_LIBDIR` environment variable. The `pcap`
   crate's build script adds it to the linker search path, and `wpcap.lib` is then resolved by
   the crate's own `#[link(name = "wpcap")]`:

   ```powershell
   $env:LIBPCAP_LIBDIR = 'C:\Npcap-SDK\Lib\x64'
   ```

   To make it permanent for your user account:

   ```powershell
   [Environment]::SetEnvironmentVariable('LIBPCAP_LIBDIR', 'C:\Npcap-SDK\Lib\x64', 'User')
   ```

   Alternatively, instead of the environment variable, add a linker search path in
   `.cargo/config.toml` (project-local or `%USERPROFILE%\.cargo\config.toml`):

   ```toml
   [target.x86_64-pc-windows-msvc]
   rustflags = ["-L", "native=C:\\Npcap-SDK\\Lib\\x64"]
   ```

> A modern Npcap SDK `wpcap.lib` is self-contained; you do **not** need to add `Packet.lib`
> explicitly. An old **WinPcap** Developer Pack (`C:\WpdPack`) will *not* work: its `wpcap.lib`
> lacks `pcap_set_immediate_mode`, `pcap_set_tstamp_type` and friends, so linking fails with
> `LNK2019: unresolved external symbol pcap_*`.

### 2. Npcap runtime — required to run

Install the **Npcap runtime** — the `.exe` installer from <https://npcap.com/#download>, which is
separate from the SDK. During installation:

- Tick **"Install Npcap in WinPcap API-compatible Mode"**.
- Leave **"Restrict Npcap driver's access to Administrators only"** *unticked* if you want to run
  the example from a normal (non-elevated) shell. If you tick it, run the example from an
  **Administrator** terminal.

Without the runtime installed, the built `.exe` fails when opening the network device.

## Find your network interface name

EtherCrab needs the `\Device\NPF_{GUID}` name of the NIC wired to the EtherCAT network. The Npcap
NPF name is always `\Device\NPF_` followed by the adapter's interface GUID, so PowerShell can
print every candidate:

```powershell
Get-NetAdapter | ForEach-Object {
    '{0,-16} {1,-46} {2,-12} \Device\NPF_{3}' -f `
        $_.Name, $_.InterfaceDescription, $_.Status, $_.InterfaceGuid
}
```

Plug the EtherCAT cable in first, then pick the row whose `Status` is `Up` and that corresponds
to the port wired to the EK1100 (an EK1100 link comes up at **100 Mbps**). You can cross-check
against `getmac /fo csv /v` or Wireshark's capture-interface list.

> **If you also run TwinCAT on this machine:** having the *TwinCAT RT-Ethernet Filter Driver*
> bound to the adapter is fine on its own — `tcrtefilter` and `beckhoff_tcether` can both show
> `Enabled = True` in `Get-NetAdapterBinding` and EtherCrab still works, even with the TwinCAT
> system service running. What can take the frames is TwinCAT actively running a configuration
> that claims that NIC. If you get 0 SubDevices and TwinCAT is in Run mode, stop it, or use a
> different NIC for EtherCrab.

## Run the MainDevice

Connect an **EK1100 or EK1501 as the first SubDevice**, with any number of modules after it,
then, from PowerShell:

```powershell
$env:LIBPCAP_LIBDIR = 'C:\Npcap-SDK\Lib\x64'   # only needed if not set permanently
$env:RUST_LOG = 'info'
cargo run --release --example ek1100-windows -- '\Device\NPF_{YOUR-GUID-HERE}'
```

Use `--release`; a debug build has poor cycle timing.

### Expected output

```text
[INFO ek1100_windows] Starting EK1100/EK1501 demo (Windows)...
[INFO ek1100_windows] Discovered 3 SubDevices
[INFO ek1100_windows] -> SubDevice 0x1000 EK1100 inputs: 0 bytes, outputs: 0 bytes
[INFO ek1100_windows] -> SubDevice 0x1001 EL2008 inputs: 0 bytes, outputs: 1 bytes
[INFO ek1100_windows] -> SubDevice 0x1002 EL6910 inputs: 2 bytes, outputs: 2 bytes
[INFO ek1100_windows] Cycling process data. Press Ctrl + C to stop.
```

1. The discovered SubDevices are logged.
2. After the transition to `OP`, each SubDevice's input / output byte counts are printed.
3. The process-data loop runs on a 5 ms cycle, incrementing every output byte by one each cycle
   (so e.g. the outputs of an EL2xxx digital-output module visibly toggle).
4. Press **Ctrl + C** for a clean shutdown: `OP -> SAFE-OP -> PRE-OP -> INIT`.

Set `RUST_LOG=ethercrab=debug` or `ethercrab=trace` for protocol-level detail.

## Troubleshooting

| Symptom | Cause / fix |
| --- | --- |
| `LNK2019: unresolved external symbol pcap_*` | Npcap SDK not on the linker search path (`LIBPCAP_LIBDIR` unset or wrong), or an old WinPcap SDK is being picked up. See *Npcap SDK*. |
| Build succeeds, panic on start opening the device / "No such device exists" | Npcap **runtime** not installed, or the wrong `\Device\NPF_{...}` name. If Npcap was installed in admin-only mode, run from an elevated shell. |
| `Discovered 0 SubDevices` / `Timeout(Pdu)` at init | Cable on the wrong port, EK1100 not powered, or TwinCAT is running a configuration that claims that NIC (see *Find your network interface name*). |
| Lots of `mailbox` / status-transition timeouts | Largely expected on Windows; the example already sets `wait_loop_delay: Duration::ZERO`. See [`windows-tuning.md`](./windows-tuning.md). |
| High jitter / missed cycles | Windows is not a realtime OS. See [`windows-tuning.md`](./windows-tuning.md) for core isolation, NIC tweaks and thread priority. |
