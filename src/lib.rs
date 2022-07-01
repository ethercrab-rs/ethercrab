// pub mod pdu;
pub mod pdu2;
pub mod register;

use mac_address::MacAddress;
use nom::{bytes::complete::take, multi::many1, number::complete::le_u16};
use pdu2::Pdu;
// use pdu::{Pdu, PduParseError};
use smoltcp::wire::{EthernetAddress, EthernetFrame, EthernetProtocol};
use std::io::{self, Write};

const LEN_MASK: u16 = 0b0000_0111_1111_1111;
const ETHERCAT_ETHERTYPE: u16 = 0x88A4;

pub trait PduData {
    const LEN: u16;

    fn len() -> u16 {
        Self::LEN & LEN_MASK
    }
}

impl PduData for u8 {
    const LEN: u16 = Self::BITS as u16 / 8;
}
impl PduData for u16 {
    const LEN: u16 = Self::BITS as u16 / 8;
}
impl PduData for u32 {
    const LEN: u16 = Self::BITS as u16 / 8;
}
impl PduData for u64 {
    const LEN: u16 = Self::BITS as u16 / 8;
}
impl PduData for i8 {
    const LEN: u16 = Self::BITS as u16 / 8;
}
impl PduData for i16 {
    const LEN: u16 = Self::BITS as u16 / 8;
}
impl PduData for i32 {
    const LEN: u16 = Self::BITS as u16 / 8;
}
impl PduData for i64 {
    const LEN: u16 = Self::BITS as u16 / 8;
}
impl<const N: usize> PduData for [u8; N] {
    const LEN: u16 = N as u16;
}
/// A "Visible String" representation. Characters are specified to be within the ASCII range.
impl<const N: usize> PduData for heapless::String<N> {
    const LEN: u16 = N as u16;
}

// #[derive(Debug)]
// pub struct EthercatPduFrame {
//     /// Length of PDUs in this frame (excludes the packed u16 header)
//     len: u16,
//     pdus: Vec<pdu::Pdu>,
// }

// impl EthercatPduFrame {
//     pub fn new() -> Self {
//         Self {
//             len: 0,
//             pdus: Vec::new(),
//         }
//     }

//     // TODO: I don't think we really need to be able to support multiple PDUs in an EtherCAT frame,
//     // at least not for now.
//     pub fn push_pdu(&mut self, pdu: pdu::Pdu) {
//         self.pdus.last_mut().map(|last| {
//             last.set_has_next(true);
//         });

//         self.len += pdu.byte_len();

//         self.pdus.push(pdu);
//     }

//     pub fn as_bytes(&self, mut buf: &mut [u8]) -> io::Result<()> {
//         let packed = {
//             let len = self.len & LEN_MASK;

//             assert_eq!(len, self.len, "Length was truncated");

//             // TODO: Const 0x01 = DLPDUs
//             let protocol_type = 0x01u16 << 12;

//             len | protocol_type
//         };

//         buf.write_all(&packed.to_le_bytes())?;

//         for pdu in &self.pdus {
//             pdu.as_bytes(buf)?;
//         }

//         Ok(())
//     }

//     /// Length of this entire struct in bytes
//     pub fn byte_len(&self) -> u16 {
//         let static_len = 2;

//         dbg!(static_len + self.len)
//     }

//     pub fn as_ethernet_frame(
//         &self,
//         my_mac: MacAddress,
//         dest_mac: MacAddress,
//         buf: &mut [u8],
//     ) -> io::Result<()> {
//         let mut frame = EthernetFrame::new_checked(buf).expect("Frame");

//         self.as_bytes(&mut frame.payload_mut())?;
//         frame.set_ethertype(EthernetProtocol::Unknown(ETHERCAT_ETHERTYPE));
//         frame.set_dst_addr(EthernetAddress::from_bytes(&dest_mac.bytes()));
//         frame.set_src_addr(EthernetAddress::from_bytes(&my_mac.bytes()));

//         Ok(())
//     }

//     /// Testing only.
//     pub fn create_ethernet_buffer(&self) -> Vec<u8> {
//         let buf_len = EthernetFrame::<&[u8]>::buffer_len(self.byte_len().into());

//         dbg!(buf_len);

//         let mut frame_buf = Vec::with_capacity(buf_len);
//         frame_buf.resize(buf_len, 0x00u8);

//         frame_buf
//     }

//     pub fn parse_response<'a, 'b>(&'a self, i: &'b [u8]) -> Result<Self, PduFrameParseError> {
//         let (i, packed) = le_u16(i)?;

//         let len = packed & LEN_MASK;

//         let (i, pdus) = self
//             .pdus
//             .iter()
//             .fold((i, Vec::<Pdu>::new()), |(i, mut pdus), pdu| {
//                 let (i, pdu) = pdu.parse_response(i).unwrap();

//                 pdus.push(pdu);

//                 (i, pdus)
//             });

//         if !i.is_empty() {
//             return Err(PduFrameParseError::Incomplete);
//         }

//         Ok(Self { len, pdus })
//     }
// }

// #[derive(Debug)]
// // TODO: thiserror
// pub enum PduFrameParseError {
//     Frame,
//     Pdu(PduParseError),
//     Parse(String),
//     Incomplete,
// }

// impl From<nom::Err<nom::error::VerboseError<&[u8]>>> for PduFrameParseError {
//     fn from(e: nom::Err<nom::error::VerboseError<&[u8]>>) -> Self {
//         match e.clone() {
//             nom::Err::Incomplete(_) => todo!(),
//             nom::Err::Error(e) => {
//                 for (slice, error) in e.errors {
//                     println!("Failed to parse: {error:?}\n{slice:02x?}");
//                 }
//             }
//             nom::Err::Failure(_) => todo!(),
//         }

//         Self::Parse(e.to_string())
//     }
// }

// #[cfg(test)]
// mod tests {
//     #[test]
//     fn it_works() {
//         let result = 2 + 2;
//         assert_eq!(result, 4);
//     }
// }
