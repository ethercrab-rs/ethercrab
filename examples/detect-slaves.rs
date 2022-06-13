//! Detect slaves by reading the working counter value in the returned packet

use ethercrab::{
    pdu::{Brd, Pdu},
    EthercatPduFrame,
};
use mac_address::{get_mac_address, MacAddress};
use smoltcp::wire::{EthernetFrame, PrettyPrinter};
use std::{io, time::Instant};

#[cfg(not(target_os = "windows"))]
fn main() -> io::Result<()> {
    use smoltcp::{
        phy::{Device, RxToken},
        wire::{EthernetFrame, PrettyPrinter},
    };

    smol::block_on(async {
        let medium = smoltcp::phy::Medium::Ethernet;
        let sock = smoltcp::phy::RawSocket::new("lo", medium).unwrap();

        let mut listener = async_io::Async::<smoltcp::phy::RawSocket>::new(sock).unwrap();

        while let Ok(frame) = listener
            .read_with_mut(|raw_socket| {
                let (rx, _tx) = raw_socket
                    .receive()
                    .ok_or_else(|| io::ErrorKind::WouldBlock)?;

                let frame = rx
                    .consume(Instant::now().into(), |buffer| {
                        let frame =
                            PrettyPrinter::<EthernetFrame<&[u8]>>::new("", &buffer).to_string();

                        Ok(frame)
                    })
                    .map_err(smoltcp_to_io)?;

                Ok(frame)
            })
            .await
        {
            println!("Recv {}", frame);
        }

        Ok(())
    })
}

#[cfg(target_os = "windows")]
fn main() -> io::Result<()> {
    use pnet::{
        datalink,
        packet::{ethernet::EthernetPacket, Packet},
    };

    dbg!(datalink::interfaces());

    let src = get_mac_address()
        .expect("Failed to read MAC")
        .expect("No mac found");

    let interface_name = "\\Device\\NPF_{0D792EC2-0E89-4AB6-BE39-3F41EC42AEA3}";

    let interfaces = datalink::interfaces();
    let interface = interfaces
        .into_iter()
        .find(|interface| interface.name == interface_name)
        .unwrap();

    let (mut tx, mut rx) = match datalink::channel(&interface, Default::default()) {
        Ok(datalink::Channel::Ethernet(tx, rx)) => (tx, rx),
        Ok(_) => panic!("Unhandled channel type"),
        Err(e) => panic!(
            "An error occurred when creating the datalink channel: {}",
            e
        ),
    };

    let brd = Brd::new(1, 0x0111);

    let read = broadcast_read(&brd);
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

                    let brd_response = brd.parse_response(packet.payload());

                    match brd_response {
                        Ok(response) => {
                            println!("Response! {:?}", response);
                        }
                        Err(e) => println!("Error: {e:?}"),
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

    Ok(())
}

fn broadcast_read(brd: &Brd) -> Vec<u8> {
    let mut frame = EthercatPduFrame::new();

    // Values hard coded to match Wireshark capture
    frame.push_pdu(Pdu::Brd(brd.clone()));

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
