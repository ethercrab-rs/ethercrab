pub mod abort_code;
pub mod services;

use packed_struct::{prelude::*, PackingResult};

/// Defined in ETG1000.6 5.6.1
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CoeHeader {
    pub service: CoeService,
}

impl PackedStruct for CoeHeader {
    type ByteArray = [u8; 2];

    fn pack(&self) -> PackingResult<Self::ByteArray> {
        // NOTE: The spec hard codes this value for every CoE service to 0x00, however it's a
        // defined field so I'll leave it in the code to hopefully make things a bit clearer when
        // referring to the spec.
        let number = 0;
        let number = number & 0b1_1111_1111;

        let service = self.service as u16;

        let raw = number | (service << 12);

        Ok(raw.to_le_bytes())
    }

    fn unpack(src: &Self::ByteArray) -> PackingResult<Self> {
        let raw = u16::from_le_bytes(*src);

        let service =
            CoeService::from_primitive((raw >> 12) as u8).ok_or(PackingError::InvalidValue)?;

        Ok(Self { service })
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
#[derive(Clone, Copy, Debug, PartialEq, Eq, PackedStruct)]
#[packed_struct(size_bytes = "1", bit_numbering = "lsb0", endian = "lsb")]
pub struct InitSdoFlags {
    #[packed_field(bits = "0")]
    pub size_indicator: bool,
    #[packed_field(bits = "1")]
    pub expedited_transfer: bool,
    #[packed_field(bits = "2..=3")]
    pub size: u8,
    #[packed_field(bits = "4")]
    pub complete_access: bool,
    #[packed_field(bits = "5..=7")]
    pub command: u8,
}

impl InitSdoFlags {
    pub const DOWNLOAD_REQUEST: u8 = 0x01;
    // pub const DOWNLOAD_RESPONSE: u8 = 0x03;
    pub const UPLOAD_REQUEST: u8 = 0x02;
    // pub const UPLOAD_RESPONSE: u8 = 0x02;
    pub const ABORT_REQUEST: u8 = 0x04;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PackedStruct)]
#[packed_struct(size_bytes = "4", bit_numbering = "msb0", endian = "lsb")]
pub struct InitSdoHeader {
    #[packed_field(bytes = "0")]
    pub flags: InitSdoFlags,
    #[packed_field(bytes = "1..=2")]
    pub index: u16,
    #[packed_field(bytes = "3")]
    pub sub_index: u8,
}

/// Defined in ETG1000.6 5.6.2.3.1
#[derive(Clone, Copy, Debug, PartialEq, Eq, PackedStruct)]
#[packed_struct(size_bytes = "1", bit_numbering = "lsb0", endian = "lsb")]
pub struct SegmentSdoHeader {
    #[packed_field(bits = "0")]
    pub is_last_segment: bool,
    #[packed_field(bits = "1..=3")]
    /// Segment data size, `0x00` to `0x07`.
    pub segment_data_size: u8,
    #[packed_field(bits = "4")]
    pub toggle: bool,
    #[packed_field(bits = "5..=7")]
    command: u8,
}

impl SegmentSdoHeader {
    // const DOWNLOAD_SEGMENT_REQUEST: u8 = 0x00;
    // const DOWNLOAD_SEGMENT_RESPONSE: u8 = 0x01;
    const UPLOAD_SEGMENT_REQUEST: u8 = 0x03;
    // const UPLOAD_SEGMENT_RESPONSE: u8 = 0x03;
}

/// Subindex access.
#[derive(Copy, Clone, Debug)]
pub enum SubIndex {
    /// Complete access.
    ///
    /// Accesses the entire entry as a single slice of data.
    Complete,

    /// Individual sub-index access.
    Index(u8),
}

impl SubIndex {
    pub(crate) fn complete_access(&self) -> bool {
        matches!(self, Self::Complete)
    }

    pub(crate) fn sub_index(&self) -> u8 {
        match self {
            // 0th sub-index counts number of sub-indices in object, so we'll start from 1
            SubIndex::Complete => 1,
            SubIndex::Index(idx) => *idx,
        }
    }
}
