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

ETG1000.4 Section 8.2.7.2 Delay measurement shows the delay measurement process.

The EtherCAT poster describes the setups steps quite nicely.

In SOEM, `ec_configdc` seems to transparently inject some behaviour into PDU sends; commenting it
out of `simple_test.c` stops cyclic DC output.

DC registers start at 0x0900 - see ETG1000.4 Table 60

System time is nanosecond-based.

The DCSM in ETG100.4 8.2.7 describes the configuration process:

- Add a flag for each slave to show whether it supports DC or not during config.
  - This is read from `RegisterAddress::SupportFlags` (`0x0008`). Return type is also defined in
    `register.rs` as `struct SupportFlags`.
- Also add flag whether the slave uses 32 bit or 64 bit DC time. Read this from
  `SupportFlags::has_64bit_dc`
  - We'll error out for 32 bit time for now
- Get system time, offset so epoch is 2000-01-01 00:00:00 as per EtherCAT spec. Convert to
  nanoseconds.
  - For embedded, this could just be 0, maybe?
- `BWR` to register `0x0900` (Receive time port 0) - ETG1000.4 Table 60
  - WKC should be the same as the number of slaves that support DC
  - NOTE: The table specifies readonly, but a write will latch the time into it.
  - This latches the receive time in each slave's
- For all slaves

  - Read register `0x0918` - "Receive time processing unit", whatever that means
  - Subtract that value from the system time
  - Write new value to `0x0920` - system time offset

- Use `FRMW` to distribute this first slave's clock to everything else. Presumably slaves
  self-filter for DC support.

Limitations for now:

- Assume entry port is port 0. This will need to change if we have different topologies and/or
  redundant NICs going backwards.

// ETG1000.4 Section 5.4.3.7 Table 27 – Configured address physical read multiple write (FRMW)

There is also an excellent comment from SOEM
[here](https://github.com/OpenEtherCATsociety/SOEM/issues/487#issuecomment-786245585)

Webinar here: <https://www.youtube.com/watch?v=1upqqL_kBmw&list=PLAMqSJyfMRtiw6EiFdHCe6JFFgRjTZGCo>

Webinar notes

- The slave latches the time between seeing the frame during TX and seeing the frame during RX.

  - Hence `/2` to calculate one way delay between devices
  - This would be the receive time on port 0 and the receive time on the last open port on the slave
    (i.e. the return port).
  - This won't necessarily be the entire network. If we're in a branch of a tree for example, the
    measured packet will only traverse to the leaf node.

- We could represent the propagation delays as a tree
- To calculate the delay between two nodes, we take the parent, subtract its propagation delay from
  the current node's delay and divide it by two.
- We need to know the entire tree structure to compute propagation delays correctly
  - `SlaveGroupContainer` needs a way to get/establish/whatever slave relationships.

ETG1000.4 Table 61 mentions DC user P1 to DC user P12 - these are defined in ETG1000.6 Table 27

## `sync0`

- [This comment](https://github.com/OpenEtherCATsociety/SOEM/issues/142#issuecomment-356661080)
  heavily implies each slave that wants to work with sync0 should set it up, not just the first one.
  - [This issue](https://github.com/OpenEtherCATsociety/SOEM/issues/618) corroborates that.
  - As does the code in `main.zip` [here](https://github.com/OpenEtherCATsociety/SOEM/issues/585)
- [This](https://infosys.beckhoff.com/english.php?content=../content/1033/ethercatsystem/2469122443.html&id=)
  is a good page on synchronisation modes.
- A decent rule of thumb for Sync0 offset is half the cycle time - if the cycle time is long enough.

## Topology

Helpful webinar:
<https://www.youtube.com/watch?v=1upqqL_kBmw&list=PLAMqSJyfMRtiw6EiFdHCe6JFFgRjTZGCo>

ETG1000.3 Section 4.2 Topology:

> An Ethernet frame received on port n (n not zero) is forwarded to port n+1. If there is no port
> n+1, the Ethernet frame is forwarded to port 0. If no device is connected or the port is closed by
> the master, a request to send to that port will be processed as if the same data are received by
> this port (i.e. loop is closed).

Also some discussion in ETG1000.3 Section 4.7.2 EtherCAT modes

A tree topology can be formed if a node has more than one port.

If a port is in loopback, it simply means data is passed straight through it.

## Sync modes - free run

ETG1020 Table 69: 0x1C32 Free Run shows values to read from SDOs to verify/monitor/set run mode.

The types for these values (or some of them anyway) are defined in ETG1000.6 Table 78 – Sync Manager
Synchronization

We also have 0x1C33 which is for inputs.

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
  - Sub-indices return type (PDO read/write, etc)
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

# Topology

- Only _needed_ for DC, otherwise a nice to have to print it out.
- For DC purposes, what is the parent of a node?
  - ~~DC propagation delay times how long a packet takes to transit from a node's port 0 back
    through its next active port, ordered like 0 -> 3 -> 1 -> 2~~
- Need to walk up the tree of nodes to compute DC from the first DC supporting node
  - Walking up the tree, do the following until we find a DC slave, but in a loop:
    - If the previous node has 2 ports open, follow back along port 0
    - If the previous node has 3 ports open (e.g. EK1100),
- Open port counts denote type of topology:
  1. End of line
  2. Normal chain link
  3. Node has children
  4. Node has a cross structure, i.e. data comes into port 0, around port 3, around port 2, around
     port 1 then back out through port 0
- SOEM's logic in `ecx_config_init`, roughly `ethercatconfig.c:440`
  - Topology number (`h` variable) is just number of open ports
  - Look for previous
    - If previous is an end point (e.g. EL1004)
      - Continue iteration backwards until we meet either the master or a slave with 3 or 4 open
        ports
    - Else if previous is a 2 port
      - Assign previous to current parent index and exit loop

## Delay calculation

- Previous slave's propagation delay minus the current one's
- If previous slave has children (a fork only), subtract that time from its delay. This turns the
  delay calculation from going through both branches to just the branch with the current slave in
  it, e.g. EK1100 followed by LAN9252 should remove the delay of the EL2004, etc.
- TODO: Instead of just the previous slave, walk back up the slave list and find the first
  DC-supporting slave.

# DS402/CiA402/MDP

ETG5000.1 specifies MDP

We also have a mention to it in `/EtherCATInfo/Descriptions/Devices/Device/Profile/AddInfo` of
ETG2000 (and previous)

SDO 0x1000 should equal `5001` for MDP, `402` for CiA402.

ETG1000.6 section 5.6.7.4.1 Device Type specifies type for 0x1000 - it's a `u32`. Note however that
this is split into two `u16`. As per ETG2000 page 60:

- `/EtherCATInfo/Descriptions/Devices/Device/Profile/ProfileNo`

  > Number of the used device profile of this device (e.g., “5001” for MDP or “402” for CiA402 servo
  > drives). The device profile is also specified in CoE object “Device Type” (0x1000 Bit 0-15)

- `/EtherCATInfo/Descriptions/Devices/Device/Profile/AddInfo`

  > Additional information (depending on device profile) number of the device, e.g., according to
  > ETG.5001 MDP sub-profile types. The additional information number is also specified in CoE
  > object “Device Type” (0x1000 Bit 16-32). In case of the ChannelCount element is used to specify
  > channels of the device (value != 0) the additional information is considered as “0”. Instead,
  > the value of the element is used as channel profile value for all n channels (n = value of the
  > value of the element is used as channel profile value for all n channels (n = value ofh
  > ChannelCount). The profile channel numbers are also specified in CoE object entries (0xF01m:nn
  > Bit 0-15). One entry for each channel. If ProfileNo = 402, Default Value = 2 (Servo drives).

MDP (Modular Device Profile) information is read from SDO 0xf000 onwards - defined in ETG5001
section 4.2.3.2. A slave can have multiple profiles.

ETG5001.1 Figure 5 shows a good overview of address mappings

ETG6010 Table 85: Object dictionary has a list of all addresses to their purposes, e.g.

- 0x1001 error register
- 0x6040 control word
- 0x6041 status word

I _think_ TwinCAT reads profiles from ESI or does some such other magic - I can't find anything in
either WireShark or the specs that allows reading of drive parameters to put it in CSP or CSV or
whatever.

The LinuxCNC comp uses an XML file to configure mappings, e.g.
<https://github.com/dbraun1981/hal-cia402/blob/main/example/ethercat-conf.xml> which is definitely
not magic. We can do this in our PO2SO hook then.

## Resolution calculation

ETG6010 section 13.2 describes resolution calculation for stepper motors. `608fh` holds encoder
resolution. AKD prints `1048576` for this register.

AKD manual section 5.3.74 Object 608Fh: Position encoder resolution (DS402) has a bit more info

60E6h and 60EBh have similar structures to 608Fh, but counts/revs are split apart.
