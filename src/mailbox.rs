use crate::coe::{CoeHeader, SdoHeader};
use packed_struct::prelude::*;

#[derive(Default, Copy, Clone, Debug, PartialEq, Eq, PrimitiveEnum_u8)]
#[repr(u8)]
pub enum Priority {
    #[default]
    Lowest = 0x00,
    Low = 0x01,
    High = 0x02,
    Highest = 0x03,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, PrimitiveEnum_u8)]
#[repr(u8)]
pub enum MailboxType {
    /// error (ERR)
    Err = 0x00,
    /// ADS over EtherCAT (AoE)
    Aoe = 0x01,
    /// Ethernet over EtherCAT (EoE)
    Eoe = 0x02,
    /// CAN application protocol over EtherCAT (CoE)
    Coe = 0x03,
    /// File Access over EtherCAT (FoE)
    Foe = 0x04,
    /// Servo profile over EtherCAT (SoE)
    Soe = 0x05,
    // 0x06 -0x0e: reserved
    /// Vendor specific
    VendorSpecific = 0x0f,
}

#[derive(Clone, Debug, PartialEq, Eq, PackedStruct)]
#[packed_struct(size_bytes = "6", bit_numbering = "msb0", endian = "lsb")]
pub struct MailboxHeader {
    /// Mailbox data payload length.
    #[packed_field(bytes = "0..=1")]
    length: u16,
    #[packed_field(bytes = "2..=3")]
    address: u16,
    // reserved6: u8,
    #[packed_field(bits = "38..=39", ty = "enum")]
    priority: Priority,
    // #[packed_field(bits = "40..=43", ty = "enum")]
    #[packed_field(bits = "44..=47", ty = "enum")]
    mailbox_type: MailboxType,
    // #[packed_field(bits = "44..=46")]
    #[packed_field(bits = "41..=43")]
    counter: u8,
    // _reserved1: u8
}

// TODO: Rename to `CoEMailboxHeader`? I'll have to see what happens when other protocols are
// implemented.
#[derive(Clone, Debug, PartialEq, Eq, PackedStruct)]
#[packed_struct(size_bytes = "25", bit_numbering = "msb0", endian = "lsb")]
struct Mailbox {
    #[packed_field(bytes = "0..=5")]
    header: MailboxHeader,
    #[packed_field(bytes = "6..=7")]
    coe_header: CoeHeader,
    #[packed_field(bytes = "8..=11")]
    sdo_header: SdoHeader,
    #[packed_field(bytes = "12..25")]
    // "complete size DWORD" + 10 bytes of data
    data: [u8; 14],
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coe::{CoeService, SdoFlags, SdoHeader};
    use chrono::Utc;
    use pcap::{Capture, Linktype, Packet, PacketHeader};
    use std::path::PathBuf;

    fn write_bytes_to_file(name: &str, data: &[u8]) {
        let mut frame = crate::pdu_loop::pdu_frame::Frame::default();

        frame
            .replace(
                crate::command::Command::Fpwr {
                    address: 0x1001,
                    register: 0x1800,
                },
                data.len() as u16,
                0xaa,
            )
            .unwrap();

        let mut buffer = Vec::with_capacity(1536);
        buffer.resize(1536, 0);

        frame
            .to_ethernet_frame(buffer.as_mut_slice(), data)
            .unwrap();

        // Epic haxx: force length header param to 1024. This should be the mailbox buffer size
        buffer.as_mut_slice()[0x16] = 0x00;
        buffer.as_mut_slice()[0x17] = 0x04;

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

        let path = PathBuf::from(&name);

        let mut save = cap.savefile(&path).expect("Open save file");

        save.write(&packet);
        drop(save);
    }

    #[test]
    fn encode_header() {
        // From wireshark capture
        let expected = [0x0a, 0x00, 0x00, 0x00, 0x00, 0x33];

        let packed = MailboxHeader {
            length: 10,
            priority: Priority::Lowest,
            address: 0x0000,
            counter: 3,
            mailbox_type: MailboxType::Coe,
        }
        .pack()
        .unwrap();

        assert_eq!(packed, expected);
    }

    #[test]
    fn decode_header() {
        // From Wireshark capture "soem-slaveinfo-akd.pcapng", packet #296
        let raw = [0x0a, 0x00, 0x00, 0x00, 0x00, 0x23];

        let expected = MailboxHeader {
            length: 10,
            address: 0x0000,
            priority: Priority::Lowest,
            mailbox_type: MailboxType::Coe,
            counter: 2,
        };

        let parsed = MailboxHeader::unpack(&raw).unwrap();

        assert_eq!(parsed, expected);
    }

    #[test]
    fn decode_mailbox_upload_response() {
        // From Wireshark capture "soem-single-lan9252.pcapng", packet #301
        let raw = [
            0x0au8, 0x00, 0x00, 0x00, 0x00, 0x13, 0x00, 0x20, 0x50, 0x00, 0x1c, 0x00, 0x00, 0x00,
            0x00, 0x00, //
            // Data
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ];

        let expected = Mailbox {
            header: MailboxHeader {
                length: 10,
                address: 0x0000,
                priority: Priority::Lowest,
                mailbox_type: MailboxType::Coe,
                counter: 1,
            },
            coe_header: CoeHeader {
                number: 0,
                service: CoeService::SdoRequest,
            },
            sdo_header: SdoHeader {
                flags: SdoFlags {
                    // The size field does NOT indicate the data size
                    size_indicator: false,
                    expedited_transfer: false,
                    size: 0,
                    complete_access: true,
                    // 0x02 = initiate upload
                    // TODO: consts
                    command: 0x02,
                },
                index: 0x1c00,
                sub_index: 0,
            },
            data: [0u8; 14],
        };

        let parsed = Mailbox::unpack(&raw).unwrap();

        assert_eq!(parsed, expected);
    }

    #[test]
    fn encode_mailbox_frame() {
        // Download expedited response (ETF1000.6 table 31)
        let mbox = Mailbox {
            header: MailboxHeader {
                length: 10,
                address: 0x0000,
                priority: Priority::Lowest,
                mailbox_type: MailboxType::Coe,
                counter: 2,
            },
            coe_header: CoeHeader {
                number: 0,
                service: CoeService::SdoRequest,
            },
            sdo_header: SdoHeader {
                flags: SdoFlags {
                    // The size field does NOT indicate the data size
                    size_indicator: false,
                    expedited_transfer: false,
                    size: 0,
                    complete_access: false,
                    command: 0x02,
                },
                index: 0x1c12,
                sub_index: 0,
            },
            // data: [0x40, 0x00, 0x1c, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
            data: [0xff; 14],
        };

        let packed = mbox.pack().unwrap();

        write_bytes_to_file("mailbox-encode_mailbox_frame.pcapng", &packed);
    }
}
