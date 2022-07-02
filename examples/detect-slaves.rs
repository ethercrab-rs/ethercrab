//! Detect slaves by reading the working counter value in the returned packet

use chrono::Utc;
use ethercrab::pdu2::Pdu;
use mac_address::{get_mac_address, MacAddress};
use pcap::PacketHeader;
use pnet::{
    datalink::{self, DataLinkReceiver, DataLinkSender},
    packet::{ethernet::EthernetPacket, Packet},
};
use smoltcp::wire::{EthernetAddress, EthernetFrame, EthernetProtocol, PrettyPrinter};
use std::{io, path::PathBuf};

#[cfg(target_os = "windows")]
const INTERFACE: &str = "\\Device\\NPF_{0D792EC2-0E89-4AB6-BE39-3F41EC42AEA3}";
#[cfg(not(target_os = "windows"))]
const INTERFACE: &str = "eth0";

fn get_tx_rx() -> (Box<dyn DataLinkSender>, Box<dyn DataLinkReceiver>) {
    let interfaces = datalink::interfaces();

    dbg!(&interfaces);

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

    // TODO: Register address enum ETG1000.4 Table 31
    let pdu = Pdu::<1>::new(0x0000);

    // let mut frame = EthercatPduFrame::new();

    // // Values hard coded to match Wireshark capture
    // frame.push_pdu(Pdu::Brd(Brd::new(1, 0x0111)));

    let ethernet_frame = pdu_to_ethernet(&pdu);

    // ---

    {
        let buffer = ethernet_frame.clone().into_inner();

        let packet = pcap::Packet {
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

        let cap = pcap::Capture::dead(pcap::Linktype::ETHERNET).expect("Open capture");

        let name = std::file!().replace(".rs", ".pcapng");

        let path = PathBuf::from(&name);

        let mut save = cap.savefile(&path).expect("Open save file");

        save.write(&packet);
        drop(save);
    }

    // ---

    // let read = broadcast_read(&frame);
    tx.send_to(&ethernet_frame.as_ref(), None)
        .unwrap()
        .expect("Send");

    // TODO: Response packet timeout
    loop {
        match rx.next() {
            Ok(packet) => {
                let packet = EthernetFrame::new_unchecked(packet);

                if packet.ethertype() == EthernetProtocol::Unknown(0x88a4) {
                    // if packet.get_destination() != src.bytes() {
                    //     println!("Packet is not for us");
                    //     continue;
                    // }

                    println!("Response");

                    let brd_response = pdu.from_ethercat_frame(packet.payload());

                    dbg!(brd_response.unwrap().1);

                    // match brd_response {
                    //     Ok(response) => {
                    //         println!("Response! {:x?}", response);
                    //     }
                    //     Err(e) => println!("Error: {e:x?}"),
                    // }

                    // let frame =
                    //     PrettyPrinter::<smoltcp::wire::EthernetFrame<&[u8]>>::new("", &buffer)
                    //         .to_string();

                    // println!("{frame}");
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

// TODO: Move into crate, pass buffer in instead of returning a vec
fn pdu_to_ethernet<const N: usize>(pdu: &Pdu<N>) -> EthernetFrame<Vec<u8>> {
    let src = get_mac_address()
        .expect("Failed to read MAC")
        .expect("No mac found");

    // Broadcast
    let dest = MacAddress::default();

    let ethernet_len = EthernetFrame::<&[u8]>::buffer_len(pdu.frame_buf_len());

    let mut buffer = Vec::new();
    buffer.resize(ethernet_len, 0x00u8);

    let mut frame = EthernetFrame::new_checked(buffer).unwrap();

    pdu.write_ethernet_payload(&mut frame.payload_mut())
        .unwrap();
    frame.set_src_addr(EthernetAddress::from_bytes(&src.bytes()));
    frame.set_dst_addr(EthernetAddress::from_bytes(&dest.bytes()));
    // TODO: Const
    frame.set_ethertype(EthernetProtocol::Unknown(0x88a4));

    println!(
        "Send {}",
        PrettyPrinter::<EthernetFrame<&'static [u8]>>::new("", &frame)
    );

    frame
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
