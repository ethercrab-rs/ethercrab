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

In SOEM, `ec_configdc` seems to transparently inject some behaviour into PDU sends; commenting it
out of `simple_test.c` stops cyclic DC output.

# Reading config from EEPROM

## Sync managers

Looking through SOEM, it retrieves the sync manager data length from the `TXPDO`/`RXPDO` sections.
It keeps a list of both and references them through field 0x0003 (`SyncM`) in ETG2010 Table 14.

SOEM does a bunch of PDO config in `ecx_siiPDO`. Sync manager length is set from `eepPDO.SMbitsize`
in `ecx_map_sii`.

SOEM reads TXPDOs and RXPDOs into the same array. See `ecx_map_sii`, `Isize`/`Osize`.

SM control byte mentioned in ETG2010 Table 11 corresponds to `Sm/@ControlByte` (p96) in ETG2000,
which then points to addres 0x0800 + 0x0004, i.e. ETG1000.4 Table 58, starting from `Buffer type`
row.

## Slaves with CoE

CoE (CANOpen over EtherCAT) slaves need to map SDOs into the PDI. This is done after the preop ->
safeop hook is called so the user has a chance to configure the slave correctly.

ETG1000.5 Section 6.1.4.1.6 describes some mappings

ETG1000.6 Section 5.6.7.4 CoE Communication Area / Table 67 – CoE Communication Area lists items and
locations that can be read for configuration.

SOEM sets the slave's `Obits` and `Ibits` in `ecx_map_coe_soe`. If they're greater than zero (i.e.
we have PDI mapping from CoE), the `ecx_map_sii` call in `ecx_config_find_mappings` will be a noop.

The only thing CoE changes for sync managers is the data length. The FMMUs must be altered according
to the CoE dictionary.

> The tables are located in the object directory at index 0x1600 to 0x17FF for the RxPDOs and at
> 0x1A00 to 0x1BFF for the TxPDOs.

Note that TX/RX is from the slave's point of view.

- ETG1000.6 Section 5.6.7.4 shows the CoE comms area.
- We need to read sync manager usage from this area under index `0x1c00` -
  `Sync Manager Communication Type`
- ETG1000.6 Section 5.6.7.4.9 describes `Sync Manager Communication Type` structure
- There are a bunch of TX/RX PDO mappings also in this table.
- SOEM reads sync manager configs from the SDOs. Because `Ibits` and `Obits` are set, the SM config
  is not clobbered because the EEPROM config doesn't run.

When we write to e.g. 0x1C12 we're actually configuring a sync manager's PDO mappings, which is then
read back out of the dictionary during auto config. Pretty neat.

ETG1000.5 Section 6.1.4.1.3 SDO interactions lists different request/response scenarios.

### Configuring from SDOs

- `0x1c00` index 0 returns SM count.
  - Sub-indices return type
- CoE sync manager assignment (u16) returns an address to read which is the PDO mapping for that SM
- SMs start at 0x1c10, but we should be skipping the first two because they're now mailbox,
  therefore starts at 0x1c12
  - Stop looping over SMs when index count is zero, or if the SDO was not found (this is an error,
    catch it)
- Find the PDO mapping from that sync manager address (e.g. 0x1720)
- Read the number of mappings from index 0
- Loop through mappings. Each one is a u32 that decodes into (see ETG1000.6 Table 74 – Receive PDO
  Mapping):
  - Bit length of the mapped object
  - Subindex of mapped object
  - Index of mapped object
- TX and RX PDO mappings have identical structures
- If index or subindex are zero, this mapping is unused. SOEM doesn't increment the bit length or
  anything with unused PDOs.

# Mailbox/CANOpen

ETG1000.4 section 5.6 describes mailbox structure.

Mailbox data is sent within a PDU.

An expedited SDO is one where the data is contained in the same SDO (up to 4 bytes). If expedited is
not set, the data field denotes the size of the upload.

Read this: <https://www.can-cia.org/can-knowledge/canopen/sdo-protocol/>

Basic SDO frame structure from
<https://en.wikipedia.org/wiki/CANopen#Service_Data_Object_(SDO)_protocol#Service_Data_Object_(SDO)_protocol>

Also a pretty decent frame structure diagram here:
<https://www.ni.com/en-gb/innovations/white-papers/13/the-basics-of-canopen.html>

| Field                          | Type    | Comment                                                                                                                                  |
| ------------------------------ | ------- | ---------------------------------------------------------------------------------------------------------------------------------------- |
| Client Command Specifier (CCS) | BIT3    | 1 = initiate download, 2 = initiate upload, 3 = SDO segment upload, 4 = abort SDO transfer, 5 = SDO block upload, 6 = SDO block download |
| Reserved                       | BIT1    | -                                                                                                                                        |
| n                              | BIT2    | The number of bytes in the data part of the message which do not contain data, only valid if e and s are set                             |
| Expedited transfer             | BIT1    | 1 = expedited xfer (data contained in same frame), 0 = segmented xfer (data sent in subsequent frames)                                   |
| s                              | BIT1    | 1 = the data size is specified in `n` (if expedited xfer is set) or in the data part of the message                                      |
| Index                          | u16     | Object dictionary index                                                                                                                  |
| Subindex                       | u8      | Object dictionary sub index                                                                                                              |
| Data                           | [u8; 4] | Data                                                                                                                                     |

SOEM has `ecx_mbxsend` and `ecx_mbxreceive` in `ethercatmain.c`

It looks like `ecx_mbxsend` can send a large amount of data all at once so maybe the 4 byte payload
thing from CANOpen itself isn't relevant to CoE?

SOEM has `ecx_SDOwrite` in `ethercatcoe.c` which uses `ecx_mbxsend`/`ecx_mbxreceive`.

Expedited (4 bytes or less) requests just get a send/receive pair

SOEM calls `ecx_readPDOmapCA` or `ecx_readPDOmap` during initialisation. This then calls
`ecx_readPDOassignCA` which reads SDOs but doesn't write anything. Hm.

## SDO upload

- Request is sent like Table 35 - set `size_indicator`, `expedited_transfer`, `size` to false/0.
- Response can either be expedited with up to 4 bytes of data (Table 36)
  - In this case, just take the 4 and convert into `T`
- Or the response can return a "normal" response with up to mailbox length - 10 bytes of data
  (table 37)
  - Note that the mailbox header length changes here
  - Length field in header should have 10 subtracted from it when comparing with "complete size" to
    account for what appears to only be _some_ of the header(s). Hm.
  - If the "complete size" field is greater than the length header, this is the first part of a
    segmented upload and we need to sit in a loop after reading this data, making segment upload
    request/responses (table 38/table 39)
  - If "complete size" <= length, we don't need to loop
