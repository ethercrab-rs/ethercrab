//! Listen to frames coming through a raw socket.

use smoltcp::{
    phy::{Device, RxToken},
    wire::{EthernetFrame, PrettyPrinter},
};
use std::{io, time::Instant};

fn main() -> io::Result<()> {
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
