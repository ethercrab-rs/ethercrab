# Add EtherCAT Mailbox Gateway (ETG.8200) Support

## Summary
Adds a UDP/IP Mailbox Gateway on port **0x88A4 (34980)** for transparent forwarding of EtherCAT mailbox telegrams. This enables external tools (e.g., Beckhoff TwinSAFE Loader/User) to communicate with EtherCAT devices through EtherCrab.

## What's included
- **`SubDeviceRef::mailbox_raw_roundtrip`** — Sends a complete mailbox telegram (6‑byte header + payload) to SM0 and returns one reply from SM1. Validates header sizes, handles SM0/SM1 readiness, works in **PRE‑OP**.
- **Example:** `examples/mailbox_gateway.rs` (UDP only) — Listens on `0x88A4`, maps configured station addresses to sub‑devices, forwards telegrams transparently, and serializes requests.
- **Core tests:** `examples/mailbox_gateway_core.rs` — Header preservation, length handling, trailing‑byte tolerance, oversized replies, and serialization checks.

## Behavior
- Preserves EtherCAT header upper bits (15..11) and updates only the 11‑bit length in replies.
- Forwards exactly one mailbox telegram per request; **no reply on error/timeout**.
- Accepts frames with trailing bytes; only the logical telegram `(6 + mailbox_length)` is forwarded.

## Usage
```bash
cargo run --example mailbox_gateway <iface>
# Beckhoff tools:
TwinSAFE_Loader.exe --gw <gateway-ip> --list devices.csv
TwinSAFE_User.exe   --gw <gateway-ip> --slave <addr> --list users.csv
```

(Per the Beckhoff docs, MBG uses **UDP 0x88A4**.)

## Limitations

- **UDP only** (TCP deferred).
- Address map is built at startup; topology changes require a restart.

## Breaking changes

None (purely additive).