pub mod abort_code;
pub mod services;

/// Defined in ETG1000.6 5.6.1 Table 29 – CoE elements.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ethercrab_wire::EtherCrabWireReadWrite)]
#[cfg_attr(test, derive(arbitrary::Arbitrary))]
#[wire(bytes = 2)]
pub struct CoeHeader {
    #[wire(pre_skip = 12, bits = 4)]
    pub service: CoeService,
}

/// Defined in ETG1000.6 Table 29 – CoE elements
#[derive(Clone, Copy, Debug, PartialEq, Eq, ethercrab_wire::EtherCrabWireReadWrite)]
#[cfg_attr(test, derive(arbitrary::Arbitrary))]
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
#[derive(Clone, Copy, Debug, PartialEq, Eq, ethercrab_wire::EtherCrabWireReadWrite)]
#[wire(bytes = 1)]
pub struct InitSdoFlags {
    #[wire(bits = 1)]
    pub size_indicator: bool,
    #[wire(bits = 1)]
    pub expedited_transfer: bool,
    #[wire(bits = 2)]
    pub size: u8,
    #[wire(bits = 1)]
    pub complete_access: bool,
    #[wire(bits = 3)]
    pub command: u8,
}

impl InitSdoFlags {
    pub const DOWNLOAD_REQUEST: u8 = 0x01;
    // pub const DOWNLOAD_RESPONSE: u8 = 0x03;
    pub const UPLOAD_REQUEST: u8 = 0x02;
    // pub const UPLOAD_RESPONSE: u8 = 0x02;
    pub const ABORT_REQUEST: u8 = 0x04;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ethercrab_wire::EtherCrabWireReadWrite)]
#[wire(bytes = 4)]
pub struct InitSdoHeader {
    #[wire(bytes = 1)]
    pub flags: InitSdoFlags,
    #[wire(bytes = 2)]
    pub index: u16,
    #[wire(bytes = 1)]
    pub sub_index: u8,
}

/// Defined in ETG1000.6 5.6.2.3.1
#[derive(Clone, Copy, Debug, PartialEq, Eq, ethercrab_wire::EtherCrabWireReadWrite)]
#[wire(bytes = 1)]
pub struct SegmentSdoHeader {
    #[wire(bits = 1)]
    pub is_last_segment: bool,

    /// Segment data size, `0x00` to `0x07`.
    #[wire(bits = 3)]
    pub segment_data_size: u8,

    #[wire(bits = 1)]
    pub toggle: bool,

    #[wire(bits = 3)]
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
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
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

impl From<u8> for SubIndex {
    fn from(value: u8) -> Self {
        Self::Index(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ethercrab_wire::{EtherCrabWireRead, EtherCrabWireSized, EtherCrabWireWrite};

    #[test]
    #[cfg_attr(miri, ignore)]
    fn coe_header_fuzz() {
        heckcheck::check(|status: CoeHeader| {
            let mut buf = [0u8; { CoeHeader::PACKED_LEN }];

            let packed = status.pack_to_slice_unchecked(&mut buf);

            let unpacked = CoeHeader::unpack_from_slice(packed).expect("Unpack");

            pretty_assertions::assert_eq!(status, unpacked);

            Ok(())
        });
    }
}
