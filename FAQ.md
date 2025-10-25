# FAQ

This is a **draft**/living document and will be rough in places as it is currently just used to note
down common stub questions. It will be added to over time as more questions come in.

## How do I list available network interfaces?

### Linux

```bash
ip a
```

### macOS

```bash
networksetup -listallhardwareports
```

Example output:

```
❯ networksetup -listallhardwareports

Hardware Port: Ethernet Adapter (en3)
Device: en3
Ethernet Address: 76:0d:c4:5d:0c:e6
```

Use `en3` as command line arguments to run examples, etc.

### Windows

```batch
getmac????

TODO
```

Jeff says maybe `getmac /fo csv /v`

or:

```
Get-NetAdapter -Name * -IncludeHidden | Format-List -Property *

Select-Object Name, InterfaceGuid
```

## Does EtherCrab work on Windows?

Yes, but performance is not very good unlike Linux and to a large extend macOS, which can both do
realtime transfers.

Windows performance is potentially improved by
[`xdp-for-windows`](https://github.com/microsoft/xdp-for-windows), but EtherCrab doesn't have
support for it at time of writing.

## Does EtherCrab support ESI XML files?

Not yet. This is an often requested feature, but needs some thought into how the ESI file can
integrate into a safe API for SubDevices. This may be a derive macro, `build.rs` codegen, or
something else.

## Why won't my SubDevice go into OP?

TODO: Maybe this should be a separate "troubleshooting" doc?

- SDOs might not be configured correctly
- Some SubDevices need cyclic data before they'll go into op. Try `request_into_op` like in the
  `dc.rs` example instead of `into_op` and see if that helps.
