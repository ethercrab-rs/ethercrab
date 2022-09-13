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

# SII (slave information interface) and/or EEPROM reads

ETG1000.6 5.4 specifies SII field mappings

ETG1000.4 8.2.5.2 specifies an EEPROM read flowchart

ETG1000.4 6.4.3 specifies SII data fields for 0x0502, 0x0503

According to 6.4.4:

> The slave information interface address register contains the address in the slave information
> interface which is accessed by the next read or write operation (by writing the slave info rmation
> interface control/status register).

This seems to imply that each EEPROM address returns 2 bytes of data, hence why my test code works
with `start += 4` (half the chunk length)

SII structure, addresses, etc defined in ETG1000.6 5.4 SII Coding

## Read device name from EEPROM

ETG1000.6 Table 20 footnote states:

> NOTE 2: the first string is referenced by 1 and following, a string index of 0 refers to an empty
> string

- Read `NameIdx` (offset `0x0003`) from `General` category (ETG1000.6 5.4) into `value`
- If `value` > 0, search through strings (ETG1000.6 Table 20 – Structure Category String):
  - Find strings section start
  - Find first string length, if `index` != `value`, move offset by said length and read next
    string's length
  - Repeat until `index` == `value` or offset > strings section length
  - Read string into heapless::String
  - Return `Some(s)`
- Otherwise, return `None`

# Process data

Logical addressing is used for cyclic data - we can have up to 4GB of process data image (PDI) that
is sent with LWR/LRD/LRW.

ETG1000.3 4.8.4 Logical addressing, and further sections

# Sync manager (SM) and FMMUs

ETG1000.3 4.8.6 Sync manager introduction / ETG1000.4 6.7 Sync manager

FMMUs are used to set up mappings into the PDI

Sync managers also require configuration for the PDI to be used - by default SM2/SM3 are used for
PDI IO. SM0 and SM1 are used for mailbox comms. ETG1000.4 Table 59 footnotes specify these usages,
as shown in the tables below.

The Sync Manager channels shall be used in the following way:

| Channel | Usage                                                                                     |
| ------- | ----------------------------------------------------------------------------------------- |
| SM0     | mailbox write                                                                             |
| SM1     | mailbox read                                                                              |
| SM2     | process data write (may be used for process data read if no process data write supported) |
| SM3     | process data read                                                                         |

If mailbox is not supported, it shall be used in the following way:

| Channel                | Usage                                                                                     |
| ---------------------- | ----------------------------------------------------------------------------------------- |
| Sync Manager channel 0 | process data write (may be used for process data read if no process data write supported) |
| Sync Manager channel 1 | process data read                                                                         |

Multiple datagrams may be required if the PDI doesn't fit in one. This would be
`max(ethernet frame payload, PDU_MAX_DATA)`.

Supported mailbox protocols are at ETG1000.6 Table 18 and can be read from SII using index `0x001C`
(ETG1000.6 Table 16):

| Protocol | Value  | Description                                               |
| -------- | ------ | --------------------------------------------------------- |
| AoE      | 0x0001 | ADS over EtherCAT (routing and parallel services)         |
| EoE      | 0x0002 | Ethernet over EtherCAT (tunnelling of Data Link services) |
| CoE      | 0x0004 | CAN application protocol over EtherCAT (access to SDO)    |
| FoE      | 0x0008 | File Access over EtherCAT                                 |
| SoE      | 0x0010 | Servo Drive Profile over EtherCAT                         |
| VoE      | 0x0020 | Vendor specific protocol over EtherCAT                    |

Sync manager channel configurations are stored starting at `0x0800` - ETG1000.4 Table 59

## EL2004 - a primitive slave

Because we're assuming (TODO: Detect) a slave with no mailbox, I need to write SM0 and SM1. In SOEM
they're defined as `EC_DEFAULTMBXSM0` (`0x00010026`) and `EC_DEFAULTMBXSM1` (`0x00010022`)

SOEM does this around `ethercatconfig.c:587`.

# Distributed clocks (DC)

State machine (DCSM) described in ETG1000.4 8.2.7

Relevant registers: P1 - P12, mentioned occasionally in ETG1000.4, e.g.

> The DL-user time parameter contains the local time parameter DC user P1 to P12 for DL -user.

The next note points to ETG1000.5 but the trail goes cold when searching it for `p1`. ETG1000.4
mentions a bunch of `P*` numbers in Table 61.

ETG1000.4 6.8 also mentions DC stuff

ETG1000.4 Table 60 defines the DC stuff to start from address `0x0900`

The EtherCAT poster describes the setups steps quite nicely.
