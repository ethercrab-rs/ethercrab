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
