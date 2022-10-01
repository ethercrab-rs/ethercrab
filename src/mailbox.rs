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
    ///  vendor specific
    VendorSpecific = 0x0f,
}

#[derive(Clone, Debug, PartialEq, Eq, PackedStruct)]
#[packed_struct(size_bytes = "6", bit_numbering = "msb0", endian = "lsb")]
pub struct MailboxHeader {
    /// Mailbox data payload length.
    #[packed_field(bits = "0..=15")]
    length: u16,
    #[packed_field(bits = "16..=31")]
    address: u16,
    // reserved6: u8,
    #[packed_field(bits = "38..=39", ty = "enum")]
    priority: Priority,
    #[packed_field(bits = "40..=43", ty = "enum")]
    mailbox_type: MailboxType,
    #[packed_field(bits = "44..=47")]
    count: u8,
    // _reserved1: u8
}

pub struct Mailbox<const MAX_MAILBOX_DATA: usize> {
    header: MailboxHeader,
    data: heapless::Vec<u8, MAX_MAILBOX_DATA>,
}

impl<const MAX_MAILBOX_DATA: usize> Mailbox<MAX_MAILBOX_DATA> {
    pub fn coe(data: heapless::Vec<u8, MAX_MAILBOX_DATA>) -> Self {
        Self {
            header: MailboxHeader {
                length: data.len() as u16,
                address: 0x0000,
                priority: Priority::Lowest,
                mailbox_type: MailboxType::Coe,
                count: 0,
            },
            data,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_header() {
        // From wireshark capture
        let expected = [0x0a, 0x00, 0x00, 0x00, 0x00, 0x33];

        let packed = MailboxHeader {
            length: 10,
            priority: Priority::Lowest,
            address: 0x0000,
            count: 3,
            mailbox_type: MailboxType::Coe,
        }
        .pack()
        .unwrap();

        assert_eq!(packed, expected);
    }
}
