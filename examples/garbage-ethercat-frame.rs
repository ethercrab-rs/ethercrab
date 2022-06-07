//! Write an Ethernet packet with garbage data in it to a Wireshark capture file.

use chrono::Utc;
use ethercrab::{Aprd, EthercatPduFrame, Fprd, Pdu};
use mac_address::mac_address_by_name;
use pcap::{Capture, Linktype, Packet, PacketHeader};
use smoltcp::wire::{EthernetAddress, EthernetFrame, EthernetProtocol, PrettyPrinter};
use std::{mem, path::PathBuf};

const ETHERCAT_ETHERTYPE: u16 = 0x88A4;

#[cfg(target_os = "windows")]
const IFACE_NAME: &str = "Ethernet";
#[cfg(target_os = "macos")]
const IFACE_NAME: &str = "en0";

fn main() {
    let mut frame = EthercatPduFrame::new();

    // Values hard coded to match Wireshark capture
    frame.push_pdu(Pdu::Fprd(Fprd::new(Vec::new(), 6, 0x03e9, 0x0130)));

    let data = frame.as_bytes();

    dbg!(data.len());

    let buf_len = EthernetFrame::<&[u8]>::buffer_len(data.len());

    let mut buf = Vec::with_capacity(buf_len);
    buf.resize(buf_len, 0x00u8);

    let mut frame = EthernetFrame::new_checked(buf).expect("Frame");

    let beckhoff_mac = EthernetAddress::from_bytes(&[0x01, 0x01, 0x05, 0x01, 0x00, 0x00]);

    let my_mac = mac_address_by_name(IFACE_NAME)
        .expect("Failed to read MAC")
        .expect("No mac found");

    frame.payload_mut().copy_from_slice(&data);
    frame.set_ethertype(EthernetProtocol::Unknown(ETHERCAT_ETHERTYPE));
    frame.set_dst_addr(beckhoff_mac);
    frame.set_src_addr(EthernetAddress::from_bytes(&my_mac.bytes()));

    let buffer = frame.into_inner();

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
