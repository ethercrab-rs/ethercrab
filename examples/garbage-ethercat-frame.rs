//! Write an Ethernet packet with garbage data in it to a Wireshark capture file.

use chrono::Utc;
use cookie_factory::{
    bytes::{le_u16, le_u8},
    combinator::{skip, slice},
    gen_simple, GenError, GenResult,
};
use ethercrab::pdu2::CommandCode;
use mac_address::{get_mac_address, MacAddress};
use pcap::{Capture, Linktype, Packet, PacketHeader};
use smoltcp::wire::{EthernetAddress, EthernetFrame, EthernetProtocol, PrettyPrinter};
use std::path::PathBuf;

const ETHERCAT_ETHERTYPE: u16 = 0x88A4;

struct EthercatFrameHeader(u16);

impl EthercatFrameHeader {
    fn pdu(data_len: u16) -> Self {
        let protocol_type = 0x01;

        Self((protocol_type << 12) | (data_len & 0x0fff))
    }

    fn mailbox(data_len: u16) -> Self {
        let protocol_type = 0x05;

        Self((protocol_type << 12) | (data_len & 0b0000_0111_1111_1111))
    }

    // TODO: Network variables
}

#[derive(Debug)]
struct Datagram {
    command: u8,
    index: u8,
    auto_inc: u16,
    address: u16,
    packed: u16,
    irq: u16,
    // data: Vec<u8>,
    working_counter: u16,
}

impl Datagram {
    fn size_bytes(&self) -> u16 {
        (self.packed & 0b0000_0111_1111_1111) + 12
    }
}

fn make_register_datagram() -> Result<Vec<u8>, GenError> {
    let data = vec![0x12, 0x34];

    let datagram = Datagram {
        command: CommandCode::Brd as u8,
        index: 0,
        // Zero when sending BRD, incremented by all slaves
        auto_inc: 0,
        address: 0x123,
        packed: {
            let len = data.len() as u16;

            // No next frame; everything is zeros apart from length

            len
        },
        irq: 0,
        // data,
        // Always zero when sending from master
        working_counter: 0,
    };

    dbg!(&datagram);

    let mut buf = Vec::new();
    // +2 for frame header
    buf.resize(dbg!(datagram.size_bytes() as usize + 2), 0x00u8);

    // ---

    let frame_len = datagram.size_bytes();

    let frame_header = EthercatFrameHeader::pdu(frame_len);

    println!("{:016b}", frame_header.0);

    let working = gen_simple(le_u16(frame_header.0), buf.as_mut_slice())?;

    // ---

    let working = gen_simple(le_u8(datagram.command), working)?;
    let working = gen_simple(le_u8(datagram.index), working)?;
    let working = gen_simple(le_u16(datagram.auto_inc), working)?;
    let working = gen_simple(le_u16(datagram.address), working)?;
    let working = gen_simple(le_u16(datagram.packed), working)?;
    let working = gen_simple(le_u16(datagram.irq), working)?;
    // let working = gen_simple(skip(datagram.data.len()), working)?;
    let working = gen_simple(skip(data.len()), working)?;
    let working = gen_simple(le_u16(datagram.working_counter), working)?;

    dbg!(&buf);

    Ok(buf)
}

#[derive(Debug)]
struct Mailbox {
    data_length: u16,
    station_address: u16,
    priority: u8,
    packed: u8,
    data: Vec<u8>,
}

impl Mailbox {
    fn size_bytes(&self) -> u16 {
        6 + self.data_length
    }
}

fn make_mailbox_datagram() -> Result<Vec<u8>, GenError> {
    let data = [0x12, 0x34];

    let mailbox = Mailbox {
        // data_length: data.len() as u16,
        data_length: 64,
        station_address: 0x0000,
        priority: 0x02 << 6,
        packed: {
            // CoE
            let ty = 0x03;
            let cnt = 0;
            ty & 0x0f
        },
        data: data.to_vec(),
    };

    // ---

    let datagram = Datagram {
        command: CommandCode::Fpwr as u8,
        index: 0,
        // Zero when sending BRD, incremented by all slaves
        // ADP
        auto_inc: 0x1001,
        // ADO
        // 0x1000 or above writes into mailbox and magically makes Wireshark parse as mailbox PDU
        address: 0x1000,
        packed: {
            let len = mailbox.size_bytes();
            // No next frame; everything is zeros apart from length
            len
        },
        irq: 0,
        // data: mailbox_buf,
        // Always zero when sending from master
        working_counter: 0,
    };

    let mut buf = Vec::new();
    // +2 for frame header
    buf.resize(dbg!(datagram.size_bytes() as usize + 2), 0x00u8);

    // ---

    let frame_len = datagram.size_bytes();

    let frame_header = EthercatFrameHeader::pdu(frame_len);

    println!("{:016b}", frame_header.0);

    let working = gen_simple(le_u16(frame_header.0), buf.as_mut_slice())?;

    // ---

    let working = gen_simple(le_u8(datagram.command), working)?;
    let working = gen_simple(le_u8(datagram.index), working)?;
    let working = gen_simple(le_u16(datagram.auto_inc), working)?;
    let working = gen_simple(le_u16(datagram.address), working)?;
    let working = gen_simple(le_u16(datagram.packed), working)?;
    let working = gen_simple(le_u16(datagram.irq), working)?;

    // Mailbox body
    let working = gen_simple(le_u16(mailbox.data_length), working)?;
    let working = gen_simple(le_u16(mailbox.station_address), working)?;
    let working = gen_simple(le_u8(mailbox.priority), working)?;
    let working = gen_simple(le_u8(mailbox.packed), working)?;
    let working = gen_simple(skip(data.len()), working)?;

    let working = gen_simple(le_u16(datagram.working_counter), working)?;

    dbg!(&buf);

    Ok(buf)
}

fn write_frame(save: &mut pcap::Savefile, data: Vec<u8>) {
    // let beckhoff_mac = MacAddress::new([0x01, 0x01, 0x05, 0x01, 0x00, 0x00]);
    let beckhoff_mac = MacAddress::new([0xff; 6]);

    let my_mac = get_mac_address()
        .expect("Failed to read MAC")
        .expect("No mac found");

    let buf_len = EthernetFrame::<&[u8]>::buffer_len(data.len());
    let mut buf = Vec::with_capacity(buf_len);
    buf.resize(buf_len, 0x00u8);

    dbg!(data.len());

    let mut frame = EthernetFrame::new_checked(buf).expect("Frame");
    frame.payload_mut().copy_from_slice(&data);
    frame.set_ethertype(EthernetProtocol::Unknown(ETHERCAT_ETHERTYPE));
    frame.set_dst_addr(EthernetAddress::from_bytes(&beckhoff_mac.bytes()));
    frame.set_src_addr(EthernetAddress::from_bytes(&my_mac.bytes()));

    let buf = frame.into_inner();

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

    save.write(&packet);
}

fn main() {
    let cap = Capture::dead(Linktype::ETHERNET).expect("Open capture");

    let name = std::file!().replace(".rs", ".pcapng");

    let path = PathBuf::from(&name);

    let mut save = cap.savefile(&path).expect("Open save file");

    let register_data = make_register_datagram().unwrap();
    write_frame(&mut save, register_data);
    let mailbox_data = make_mailbox_datagram().unwrap();
    write_frame(&mut save, mailbox_data);

    drop(save);

    // // ---

    // let mut cap = Capture::from_file(&path).unwrap();

    // let _packet = cap.next();

    // // dbg!(&packet);
    // // println!("{:x?}", packet.unwrap().data);
}
