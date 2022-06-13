pub mod pdu;

use mac_address::MacAddress;
use smoltcp::wire::{EthernetAddress, EthernetFrame, EthernetProtocol};
use std::io::{self, Write};

const LEN_MASK: u16 = 0b0000_0111_1111_1111;

const ETHERCAT_ETHERTYPE: u16 = 0x88A4;

#[derive(Debug)]
pub struct EthercatPduFrame {
    /// Length of PDUs in this frame (excludes the packed u16 header)
    len: u16,
    pdus: Vec<pdu::Pdu>,
}

impl EthercatPduFrame {
    pub fn new() -> Self {
        Self {
            len: 0,
            pdus: Vec::new(),
        }
    }

    // TODO: I don't think we really need to be able to support multiple PDUs in an EtherCAT frame,
    // at least not for now.
    pub fn push_pdu(&mut self, pdu: pdu::Pdu) {
        self.pdus.last_mut().map(|last| {
            last.set_has_next(true);
        });

        self.len += pdu.byte_len();

        self.pdus.push(pdu);
    }

    pub fn as_bytes(&self, mut buf: &mut [u8]) -> io::Result<()> {
        let packed = {
            let len = self.len & LEN_MASK;

            assert_eq!(len, self.len, "Length was truncated");

            // TODO: Const 0x01 = DLPDUs
            let protocol_type = 0x01u16 << 12;

            len | protocol_type
        };

        buf.write_all(&packed.to_le_bytes())?;

        for pdu in &self.pdus {
            pdu.as_bytes(buf)?;
        }

        Ok(())
    }

    /// Length of this entire struct in bytes
    pub fn byte_len(&self) -> u16 {
        let static_len = 2;

        dbg!(static_len + self.len)
    }

    pub fn as_ethernet_frame(
        &self,
        my_mac: MacAddress,
        dest_mac: MacAddress,
        buf: &mut [u8],
    ) -> io::Result<()> {
        let mut frame = EthernetFrame::new_checked(buf).expect("Frame");

        self.as_bytes(&mut frame.payload_mut())?;
        frame.set_ethertype(EthernetProtocol::Unknown(ETHERCAT_ETHERTYPE));
        frame.set_dst_addr(EthernetAddress::from_bytes(&dest_mac.bytes()));
        frame.set_src_addr(EthernetAddress::from_bytes(&my_mac.bytes()));

        Ok(())
    }

    /// Testing only.
    pub fn create_ethernet_buffer(&self) -> Vec<u8> {
        let buf_len = EthernetFrame::<&[u8]>::buffer_len(self.byte_len().into());

        dbg!(buf_len);

        let mut frame_buf = Vec::with_capacity(buf_len);
        frame_buf.resize(buf_len, 0x00u8);

        frame_buf
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        let result = 2 + 2;
        assert_eq!(result, 4);
    }
}
