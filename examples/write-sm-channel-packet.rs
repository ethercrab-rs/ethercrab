//! Write an Ethernet packet with FMMU APWR to test packed struct.

use chrono::Utc;
use ethercrab::{
    command::Command,
    pdu::Pdu,
    sync_manager_channel::{BufferState, Control, Direction, OperationMode, SyncManagerChannel},
};
use packed_struct::{PackedStruct, PackedStructSlice};
use pcap::{Capture, Linktype, Packet, PacketHeader};
use smoltcp::wire::{EthernetFrame, PrettyPrinter};
use std::{mem::size_of, path::PathBuf};

fn main() {
    let data = SyncManagerChannel {
        physical_start_address: 0x1000,
        length: 0x0080,
        control: Control {
            buffer_type: OperationMode::Mailbox,
            direction: Direction::MasterWrite,
            ecat_event_enable: false,
            dls_user_event_enable: true,
            watchdog_enable: false,
            has_write_event: false,
            has_read_event: false,
            mailbox_full: false,
            buffer_state: BufferState::First,
            read_buffer_open: false,
            write_buffer_open: false,
        },
        enable: ethercrab::sync_manager_channel::Enable {
            channel_enabled: true,
            repeat: false,
            dc_event0w_busw: false,
            dc_event0wlocw: false,
            channel_pdi_disabled: false,
            repeat_ack: false,
        },
    };

    let mut pdu = Pdu::<8>::new(
        Command::Fpwr {
            address: 0x1234,
            register: 0x0800,
        },
        SyncManagerChannel::packed_bytes_size(None).unwrap() as u16,
        0,
    );

    let packed = data.pack().unwrap();

    println!("{packed:#04x?}");

    pdu.data = heapless::Vec::from_slice(&packed).unwrap();

    let buf_len = EthernetFrame::<&[u8]>::buffer_len(size_of::<Pdu<8>>());

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
