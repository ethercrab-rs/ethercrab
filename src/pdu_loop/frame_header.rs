//! An EtherCAT frame header.
//!
use crate::LEN_MASK;
use ethercrab_wire::{EtherCrabWireRead, EtherCrabWireSized, EtherCrabWireWrite};

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, ethercrab_wire::EtherCrabWireRead)]
#[repr(u8)]
pub(crate) enum ProtocolType {
    DlPdu = 0x01u8,
    // Not currently supported.
    // NetworkVariables = 0x04,
    // Mailbox = 0x05,
    // #[wire(catch_all)]
    // Unknown(u8),
}

/// An EtherCAT frame header.
///
/// An EtherCAT frame can contain one or more PDUs, each starting with a
/// [`PduHeader`](crate::pdu_loop::pdu_header).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct FrameHeader {
    pub(crate) payload_len: u16,
    pub(crate) protocol: ProtocolType,
}

impl EtherCrabWireSized for FrameHeader {
    const PACKED_LEN: usize = 2;

    type Buffer = [u8; 2];

    fn buffer() -> Self::Buffer {
        [0u8; 2]
    }
}

impl EtherCrabWireRead for FrameHeader {
    fn unpack_from_slice(buf: &[u8]) -> Result<Self, ethercrab_wire::WireError> {
        let raw = u16::unpack_from_slice(buf)?;

        Ok(Self {
            payload_len: raw & LEN_MASK,
            protocol: ProtocolType::try_from((raw >> 12) as u8)?,
        })
    }
}

impl EtherCrabWireWrite for FrameHeader {
    fn pack_to_slice_unchecked<'buf>(&self, buf: &'buf mut [u8]) -> &'buf [u8] {
        // Protocol in last 4 bits
        let raw = self.payload_len | (self.protocol as u16) << 12;

        raw.pack_to_slice_unchecked(buf)
    }

    fn packed_len(&self) -> usize {
        Self::PACKED_LEN
    }
}

impl FrameHeader {
    /// Create a new PDU frame header.
    pub fn pdu(len: u16) -> Self {
        debug_assert!(
            len <= LEN_MASK,
            "Frame length may not exceed {} bytes",
            LEN_MASK
        );

        Self {
            payload_len: len & LEN_MASK,
            // Only support DlPdu (for now?)
            protocol: ProtocolType::DlPdu,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pdu_header() {
        let header = FrameHeader::pdu(0x28);

        let mut buf = [0u8; 2];

        let packed = header.pack_to_slice_unchecked(&mut buf);

        let expected = &0b0001_0000_0010_1000u16.to_le_bytes();

        assert_eq!(packed, expected);
    }

    #[test]
    fn decode_pdu_len() {
        let raw = 0b0001_0000_0010_1000u16;

        let header = FrameHeader::unpack_from_slice(&raw.to_le_bytes()).unwrap();

        assert_eq!(header.payload_len, 0x28);
        assert_eq!(header.protocol, ProtocolType::DlPdu);
    }

    #[test]
    fn parse() {
        // Header from packet #39, soem-slaveinfo-ek1100-only.pcapng
        let raw = [0x3cu8, 0x10];

        let header = FrameHeader::unpack_from_slice(&raw).unwrap();

        assert_eq!(header.payload_len, 0x3c);
        assert_eq!(header.protocol, ProtocolType::DlPdu);
    }
}
