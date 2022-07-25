# Slave discovery

- Devices are discovered using an Ethernet broadcast (allballs destination MAC)
- SOEM has `ecx_detect_slaves` which sends a BRD and reads the working counter to detect number of
  slaves.
- Once the slaves have been counted by index, they can be assigned a "configured station address" by
  the master. This is done by working through the slaves by index address (SOEM uses `1 - slave`)
  and an APWR service.

  The address is 0x0010 and can be found in `ETG1000_4_V1i0i4_S_R_EcatDLLProtocols.pdf`,
  `Table 32 – Configured station address`

  SOEM (and TwinCAT) add an address offset of `0x1000` (`EC_NODEOFFSET` in SOEM land) for some
  reason.

TODO: Send a `BRD` with various devices and check WKC.

# Confirming frames

Master sets `idx` to a locally unique value and waits for a returned PDU with that same index or
until a timeout occurrs.

# CoE

CoE uses mailbox PDUs with address offset (ADO) of 0x1000-0x1FFF. ETG 1000.6 Table 62 describes this
data section.

Wireshark magically parses a PDU as CoE if the address is in that range, otherwise it parses it as a
garbage raw data read.

Frame structure for CoE goes

- `Ethernet II`
- EtherCAT frame
- Array of ethercat datagrams
  - Datagram header is common
  - Datagram data can be a register read or mailbox (CoE, etc) or PDU, the latter two I think chosen
    using the SyncManager addresses - need to confirm

# Magic register values

ETG 1000.4 has tables holding a bunch of magic addresses that do certain things (SII, DC, etc)

# MAC addressing

- EtherCAT doesn't really care about MAC address.
- Master MAC can be anything as long as we use broadcast everywhere
- The first slave connected seems to increment the first MAC octet by 2. Observed both from SOEM and
  TwinCAT. I can't find anything in the spec about this behaviour, but we can use it to filter out
  self-received packets.

# Data types

ETG 1000.4 section 5.2 Data types and encoding rules

- `u8`, `u16`, `u32`, `u64`
- `i8`, `i16`, `i32`, `i64`
- `array` (aka "octet string")
- `string` (aka "visible string") - like an array but only allows `0x20` to `0x7E`

# ESM (EtherCAT State Machine)

- <https://www.ethercat.org/memberarea/en/knowledge_base_7B6EA38CBF2349C388F6F29004196B88.htm>
- ETG1000.6 Table 102 shows ESM state table

# Initialisation

## Station address

### ETG1000.3 4.8.3.2

Use position addressing (array indexing) to initialise slave addresses to a configured station
address. Use CSA for the rest of the program.

### ETG1000.4 6.1.2/ETG1000.3 4.8.3.3

Write a u16 to register 0x0010 a configured address, can probably just be the slave index.

# AL status/control

- AL control is at `0x0120`
- AL status is at `0x0130`
- AL status code is at `0x0134`

ETG1000.6 5.3.2 lists DL-user register mappings to AL status/control/etc

ETG1000.4 6.1.5.4 which lists this mapping lists DLS-user registers and their addresses.

|             |                | Size  |
| ----------- | -------------- | ----- |
| DLS-user R1 | AL control     | `u8`  |
| DLS-user R3 | AL status      | `u8`  |
| DLS-user R6 | AL status code | `u16` |
