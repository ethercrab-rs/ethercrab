//! Write an Ethernet packet with garbage data in it to a Wireshark capture file.

use chrono::Utc;
use ethercrab::{pdu::Fprd, pdu::Pdu, EthercatPduFrame};
use mac_address::{get_mac_address, MacAddress};
use pcap::{Capture, Linktype, Packet, PacketHeader};
use smoltcp::wire::{EthernetFrame, PrettyPrinter};
use std::path::PathBuf;

// #[cfg(target_os = "windows")]
// const IFACE_NAME: &str = "Ethernet";
// #[cfg(target_os = "macos")]
// const IFACE_NAME: &str = "en0";

fn main() {
    let mut frame = EthercatPduFrame::new();

    // Values hard coded to match Wireshark capture
    frame.push_pdu(Pdu::Fprd(Fprd::new(8, 0x03e9, 0x0111)));
    frame.push_pdu(Pdu::Fprd(Fprd::new(8, 0x03e9, 0x0130)));

    let beckhoff_mac = MacAddress::new([0x01, 0x01, 0x05, 0x01, 0x00, 0x00]);

    let my_mac = get_mac_address()
        .expect("Failed to read MAC")
        .expect("No mac found");

    let mut buffer = frame.create_ethernet_buffer();

    frame
        .as_ethernet_frame(my_mac, beckhoff_mac, &mut buffer)
        .unwrap();

    println!(
        "{}",
        PrettyPrinter::<EthernetFrame<&'static [u8]>>::new("", &buffer)
    );

    // ---

    let packet = Packet {
        header: &PacketHeader {
            ts: libc::timeval {
                tv_sec: Utc::now().timestamp().try_into().expect("Time overflow"),
                tv_usec: 0,
            },
            // 64 bytes minimum frame size, minus 2x MAC address and 1x optional tag
            caplen: (buffer.len() as u32).max(46),
            len: buffer.len() as u32,
        },
        data: &buffer,
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

    // dbg!(&packet);
    // println!("{:x?}", packet.unwrap().data);
}
