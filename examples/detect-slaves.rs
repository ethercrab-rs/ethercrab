//! Detect slaves by reading the working counter value in the returned packet

use ethercrab::{
    pdu::{Brd, Pdu},
    EthercatPduFrame,
};
use mac_address::{get_mac_address, MacAddress};
use pnet::{
    datalink::{self, DataLinkReceiver, DataLinkSender},
    packet::{ethernet::EthernetPacket, Packet},
};
use smoltcp::wire::{EthernetFrame, PrettyPrinter};
use std::io;

#[cfg(target_os = "windows")]
const INTERFACE: &str = "\\Device\\NPF_{0D792EC2-0E89-4AB6-BE39-3F41EC42AEA3}";
#[cfg(not(target_os = "windows"))]
const INTERFACE: &str = "eth0";

fn get_tx_rx() -> (Box<dyn DataLinkSender>, Box<dyn DataLinkReceiver>) {
    let interfaces = datalink::interfaces();
    let interface = interfaces
        .into_iter()
        .find(|interface| dbg!(&interface.name) == INTERFACE)
        .unwrap();

    let (tx, rx) = match datalink::channel(&interface, Default::default()) {
        Ok(datalink::Channel::Ethernet(tx, rx)) => (tx, rx),
        Ok(_) => panic!("Unhandled channel type"),
        Err(e) => panic!(
            "An error occurred when creating the datalink channel: {}",
            e
        ),
    };

    (tx, rx)
}

fn main() -> io::Result<()> {
    let (mut tx, mut rx) = get_tx_rx();

    let mut frame = EthercatPduFrame::new();

    // Values hard coded to match Wireshark capture
    frame.push_pdu(Pdu::Brd(Brd::new(1, 0x0111)));

    let read = broadcast_read(&frame);
    tx.send_to(&read, None).unwrap().expect("Send");

    // TODO: Response packet timeout
    loop {
        match rx.next() {
            Ok(packet) => {
                let packet = EthernetPacket::new(packet).unwrap();

                if packet.get_ethertype() == pnet::packet::ethernet::EtherType::new(0x88a4) {
                    // if packet.get_destination() != src.bytes() {
                    //     println!("Packet is not for us");
                    //     continue;
                    // }

                    let buffer = packet.packet();

                    // TODO: Decode packet, check if it's a BRD with an `idx` of 0

                    let brd_response = frame.parse_response(packet.payload());

                    match brd_response {
                        Ok(response) => {
                            println!("Response! {:x?}", response);
                        }
                        Err(e) => println!("Error: {e:x?}"),
                    }

                    let frame =
                        PrettyPrinter::<smoltcp::wire::EthernetFrame<&[u8]>>::new("", &buffer)
                            .to_string();

                    println!("{frame}");
                }
            }
            Err(e) => {
                // If an error occurs, we can handle it here
                panic!("An error occurred while reading: {}", e);
            }
        }
    }

    // Ok(())
}

fn broadcast_read(frame: &EthercatPduFrame) -> Vec<u8> {
    let src = get_mac_address()
        .expect("Failed to read MAC")
        .expect("No mac found");

    // Broadcast
    let dest = MacAddress::default();

    let mut buffer = frame.create_ethernet_buffer();

    frame.as_ethernet_frame(src, dest, &mut buffer).unwrap();

    println!(
        "{}",
        PrettyPrinter::<EthernetFrame<&'static [u8]>>::new("", &buffer)
    );

    buffer
}

fn smoltcp_to_io(e: smoltcp::Error) -> std::io::ErrorKind {
    match e {
        smoltcp::Error::Exhausted => std::io::ErrorKind::OutOfMemory,
        // TODO: Proper mappings
        smoltcp::Error::Illegal => std::io::ErrorKind::Other,
        smoltcp::Error::Unaddressable => std::io::ErrorKind::Other,
        smoltcp::Error::Finished => std::io::ErrorKind::Other,
        smoltcp::Error::Truncated => std::io::ErrorKind::Other,
        smoltcp::Error::Checksum => std::io::ErrorKind::Other,
        smoltcp::Error::Unrecognized => std::io::ErrorKind::Other,
        smoltcp::Error::Fragmented => std::io::ErrorKind::Other,
        smoltcp::Error::Malformed => std::io::ErrorKind::Other,
        smoltcp::Error::Dropped => std::io::ErrorKind::Other,
        smoltcp::Error::NotSupported => std::io::ErrorKind::Other,
        _ => std::io::ErrorKind::Other,
    }
}
