//! Write an Ethernet packet with FMMU APWR to test packed struct.

use chrono::Utc;
use ethercrab::{command::Command, fmmu::Fmmu, pdu::Pdu};
use packed_struct::{PackedStruct, PackedStructSlice};
use pcap::{Capture, Linktype, Packet, PacketHeader};
use smoltcp::wire::{EthernetFrame, PrettyPrinter};
use std::{mem::size_of, path::PathBuf};

fn main() {
    let data = Fmmu {
        logical_start_address: 0,
        length_bytes: 1,
        logical_start_bit: 0,
        logical_end_bit: 3,
        physical_start_address: 0x1000,
        physical_start_bit: 0,
        read_enable: true,
        write_enable: false,
        enable: true,
        reserved_1: 0x00,
        reserved_2: 0x0000,
    };

    let mut pdu = Pdu::<16>::new(
        Command::Fpwr {
            address: 0x1234,
            register: 0x0600,
        },
        Fmmu::packed_bytes_size(None).unwrap() as u16,
        0,
    );

    pdu.data = heapless::Vec::from_slice(&data.pack().unwrap()).unwrap();

    let buf_len = EthernetFrame::<&[u8]>::buffer_len(size_of::<Pdu<16>>());

    let mut buf = Vec::with_capacity(buf_len);
    buf.resize(buf_len, 0x00u8);

    pdu.to_ethernet_frame(&mut buf).unwrap();

    println!(
        "{}",
        PrettyPrinter::<EthernetFrame<&'static [u8]>>::new("", &buf)
    );

    // ---

    let packet = Packet {
        header: &PacketHeader {
            ts: libc::timeval {
                tv_sec: Utc::now().timestamp().try_into().expect("Time overflow"),
                tv_usec: 0,
            },
            // 64 bytes minimum frame size, minus 2x MAC address and 1x optional tag
            caplen: (buf.len() as u32).max(46),
            len: buf.len() as u32,
        },
        data: &buf,
    };

    let cap = Capture::dead(Linktype::ETHERNET).expect("Open capture");

    let name = std::file!().replace(".rs", ".pcapng");

    let path = PathBuf::from(&name);

    let mut save = cap.savefile(&path).expect("Open save file");

    save.write(&packet);
    drop(save);

    // ---

    let mut cap = Capture::from_file(&path).unwrap();

    let _packet = cap.next();
}
