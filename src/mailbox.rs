use packed_struct::prelude::*;

#[derive(Default, Copy, Clone, Debug, PartialEq, Eq, PrimitiveEnum_u8)]
#[cfg_attr(test, derive(arbitrary::Arbitrary))]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[repr(u8)]
pub enum Priority {
    #[default]
    Lowest = 0x00,
    Low = 0x01,
    High = 0x02,
    Highest = 0x03,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, PrimitiveEnum_u8)]
#[cfg_attr(test, derive(arbitrary::Arbitrary))]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
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

/// Mailbox header.
///
/// Defined in ETG1000.6 under either `TMBXHEADER` or `MbxHeader` e.g. Table 29 - CoE Elements.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PackedStruct)]
#[packed_struct(size_bytes = "6", bit_numbering = "msb0", endian = "lsb")]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct MailboxHeader {
    /// Mailbox data payload length.
    #[packed_field(bytes = "0..=1")]
    pub length: u16,
    #[packed_field(bytes = "2..=3")]
    pub address: u16,
    // reserved6: u8,
    #[packed_field(bits = "38..=39", ty = "enum")]
    pub priority: Priority,
    #[packed_field(bits = "44..=47", ty = "enum")]
    pub mailbox_type: MailboxType,
    /// Mailbox counter from 1 to 7 inclusive. Wraps around to 1 when count exceeds 7. 0 is
    /// reserved.
    #[packed_field(bits = "41..=43")]
    pub counter: u8,
    // _reserved1: u8
}

#[cfg(test)]
mod tests {
    use super::*;
    use arbitrary::{Arbitrary, Unstructured};

    // Manual impl because `counter` field is a special case
    impl<'a> Arbitrary<'a> for MailboxHeader {
        fn arbitrary(u: &mut Unstructured<'a>) -> arbitrary::Result<Self> {
            Ok(Self {
                length: Arbitrary::arbitrary(u)?,
                address: Arbitrary::arbitrary(u)?,
                priority: Arbitrary::arbitrary(u)?,
                mailbox_type: Arbitrary::arbitrary(u)?,
                // 0..=6 shifted up by 1 so we get the valid range 1..=7
                counter: u.choose_index(7)? as u8 + 1,
            })
        }
    }

    // Keep this around so we can write test data to files for debugging
    // #[allow(unused)]
    // fn write_bytes_to_file(name: &str, data: &[u8]) {
    //     let mut frame = crate::pdu_loop::FrameElement::default();

    //     frame
    //         .replace(
    //             crate::command::Command::Fpwr {
    //                 address: 0x1001,
    //                 register: 0x1800,
    //             },
    //             data.len() as u16,
    //             0xaa,
    //         )
    //         .unwrap();

    //     let mut buffer = vec![0; 1536];

    //     frame
    //         .to_ethernet_frame(buffer.as_mut_slice(), data)
    //         .unwrap();

    //     // Epic haxx: force length header param to 1024. This should be the mailbox buffer size
    //     buffer.as_mut_slice()[0x16] = 0x00;
    //     buffer.as_mut_slice()[0x17] = 0x04;

    //     let packet = Packet {
    //         header: &PacketHeader {
    //             ts: libc::timeval {
    //                 tv_sec: Utc::now().timestamp().try_into().expect("Time overflow"),
    //                 tv_usec: 0,
    //             },
    //             // 64 bytes minimum frame size, minus 2x MAC address and 1x optional tag
    //             caplen: (buffer.len() as u32).max(46),
    //             len: buffer.len() as u32,
    //         },
    //         data: &buffer,
    //     };

    //     let cap = Capture::dead(Linktype::ETHERNET).expect("Open capture");

    //     let path = PathBuf::from(&name);

    //     let mut save = cap.savefile(&path).expect("Open save file");

    //     save.write(&packet);
    //     drop(save);
    // }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn mailbox_header_fuzz() {
        heckcheck::check(|status: MailboxHeader| {
            let packed = status.pack().expect("Pack");

            let unpacked = MailboxHeader::unpack_from_slice(&packed).expect("Unpack");

            pretty_assertions::assert_eq!(status, unpacked);

            Ok(())
        });
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
}
