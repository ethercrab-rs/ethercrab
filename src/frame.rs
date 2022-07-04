//! An EtherCAT frame.

use crate::LEN_MASK;
use nom::{error::ErrorKind, IResult};
use smoltcp::wire::EthernetFrame;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[repr(u8)]
enum ProtocolType {
    DlPdu = 0x01u8,
    NetworkVariables = 0x04,
    Mailbox = 0x05,
}

impl TryFrom<u8> for ProtocolType {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0x01 => Ok(Self::DlPdu),
            0x04 => Ok(Self::NetworkVariables),
            0x05 => Ok(Self::Mailbox),
            _ => Err(()),
        }
    }
}

#[derive(Debug)]
pub enum FrameError {
    TooLong,
    Encode(cookie_factory::GenError),
}

// TODO: Represent different behaviours with different typestates?
#[derive(Copy, Clone, Debug, PartialEq)]
#[repr(transparent)]
pub struct FrameHeader(pub u16);

impl FrameHeader {
    /// Create a new PDU frame header.
    pub fn pdu(len: usize) -> Result<Self, FrameError> {
        if len > LEN_MASK.into() {
            return Err(FrameError::TooLong);
        }

        let len = (len as u16) & LEN_MASK;

        let protocol_type = (ProtocolType::DlPdu as u16) << 12;

        Ok(Self(len | protocol_type))
    }

    /// Remove and parse an EtherCAT frame header from the given buffer.
    pub fn parse(i: &[u8]) -> IResult<&[u8], Self> {
        let (i, raw) = nom::number::complete::le_u16(i)?;

        let self_ = Self(raw);

        // TODO: Support other frame types
        if self_.protocol_type() == ProtocolType::DlPdu {
            Ok((i, self_))
        } else {
            Err(nom::Err::Error(nom::error::Error::new(
                i,
                ErrorKind::Verify,
            )))
        }
    }

    /// The length of the payload contained in this frame.
    pub fn payload_len(&self) -> u16 {
        self.0 & LEN_MASK
    }

    fn protocol_type(&self) -> ProtocolType {
        let raw = (self.0 >> 12) as u8 & 0b1111;

        raw.try_into().unwrap()
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn pdu_header() {
        let header = FrameHeader::pdu(0x28).unwrap();

        let packed = header.0;

        let expected = 0b0001_0000_0010_1000;

        assert_eq!(packed, expected, "{packed:016b} == {expected:016b}");
    }

    #[test]
    fn decode_pdu_len() {
        let raw = 0b0001_0000_0010_1000;

        let header = FrameHeader(raw);

        assert_eq!(header.payload_len(), 0x28);
        assert_eq!(header.protocol_type(), ProtocolType::DlPdu);
    }

    #[test]
    fn parse() {
        let raw = &[0b0001_0000, 0b0010_1000];

        let (rest, header) = FrameHeader::parse(raw).unwrap();

        assert_eq!(rest, &[]);

        assert_eq!(header.payload_len(), 0x28);
        assert_eq!(header.protocol_type(), ProtocolType::DlPdu);
    }
}
