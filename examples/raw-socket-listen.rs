//! Listen to frames coming through a raw socket.

use std::io;

#[cfg(not(target_os = "windows"))]
fn main() -> io::Result<()> {
    use smoltcp::{
        phy::{Device, RxToken},
        wire::{EthernetFrame, PrettyPrinter},
    };
    use std::{io::ErrorKind, time::Instant};

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
                    .map_err(|e| io::Error::new(ErrorKind::Other, e))?;

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
    use smoltcp::wire::PrettyPrinter;

    dbg!(datalink::interfaces());

    let interface_name = "\\Device\\NPF_{9E7C587C-8D88-4C37-BCEB-7ED21FC86607}";

    let interfaces = datalink::interfaces();
    let interface = interfaces
        .into_iter()
        .find(|interface| interface.name == interface_name)
        .unwrap();

    let (_tx, mut rx) = match datalink::channel(&interface, Default::default()) {
        Ok(datalink::Channel::Ethernet(tx, rx)) => (tx, rx),
        Ok(_) => panic!("Unhandled channel type"),
        Err(e) => panic!(
            "An error occurred when creating the datalink channel: {}",
            e
        ),
    };

    loop {
        match rx.next() {
            Ok(packet) => {
                let packet = EthernetPacket::new(packet).unwrap();

                if packet.get_ethertype() == pnet::packet::ethernet::EtherType::new(0x88a4) {
                    let buffer = packet.packet();

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
}
