use packed_struct::{prelude::*, PackingResult};

/// Defined in ETG1000.6 5.6.1
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CoeHeader {
    pub number: u16,
    pub service: CoeService,
}

impl PackedStruct for CoeHeader {
    type ByteArray = [u8; 2];

    fn pack(&self) -> PackingResult<Self::ByteArray> {
        let number = self.number & 0b1_1111_1111;
        let service = self.service as u16;

        let raw = number | (service << 12);

        Ok(raw.to_le_bytes())
    }

    fn unpack(src: &Self::ByteArray) -> packed_struct::PackingResult<Self> {
        let raw = u16::from_le_bytes(*src);

        let number = raw & 0b1_1111_1111;

        let service =
            CoeService::from_primitive((raw >> 12) as u8).ok_or(PackingError::InvalidValue)?;

        Ok(Self { number, service })
    }
}

/// Defined in ETG1000.6 Table 29 – CoE elements
#[derive(Clone, Copy, Debug, PartialEq, Eq, PrimitiveEnum_u8)]
#[repr(u8)]
pub enum CoeService {
    /// Emergency
    Emergency = 0x01,
    /// SDO Request
    SdoRequest = 0x02,
    /// SDO Response
    SdoResponse = 0x03,
    /// TxPDO
    TxPdo = 0x04,
    /// RxPDO
    RxPdo = 0x05,
    /// TxPDO remote request
    TxPdoRemoteRequest = 0x06,
    /// RxPDO remote request
    RxPdoRemoteRequest = 0x07,
    /// SDO Information
    SdoInformation = 0x08,
}

/// Defined in ETG1000.6 Section 5.6.2.1.1
#[derive(Clone, Debug, PartialEq, Eq, PackedStruct)]
#[packed_struct(size_bytes = "1", bit_numbering = "lsb0", endian = "lsb")]
pub struct SdoFlags {
    #[packed_field(bits = "0")]
    pub size_indicator: bool,
    #[packed_field(bits = "1")]
    pub expedited_transfer: bool,
    #[packed_field(bits = "2..=3")]
    pub size: u8,
    #[packed_field(bits = "4")]
    pub complete_access: bool,
    // TODO: Difficult to create an enum from the spec - lots of overlapping values. Maybe typos?
    // Combined with other fields? Define some `const`s perhaps.
    #[packed_field(bits = "5..=7")]
    pub command: u8,
}

#[derive(Clone, Debug, PartialEq, Eq, PackedStruct)]
#[packed_struct(size_bytes = "4", bit_numbering = "msb0", endian = "lsb")]
pub struct SdoHeader {
    #[packed_field(bytes = "0")]
    pub flags: SdoFlags,
    #[packed_field(bytes = "1..=2")]
    pub index: u16,
    #[packed_field(bytes = "3")]
    pub sub_index: u8,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pack_coe_header() {
        let header = CoeHeader {
            // number: 0b1010_1010_1,
            // number: 0x155,
            number: 0,
            service: CoeService::SdoRequest,
        };

        let packed = header.pack().unwrap();

        assert_eq!(packed, [0x00, 0x20]);
    }
}
