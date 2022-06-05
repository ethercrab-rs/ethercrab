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
