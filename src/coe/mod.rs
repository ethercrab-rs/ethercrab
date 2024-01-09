pub mod abort_code;
pub mod services;

/// Defined in ETG1000.6 Table 29 – CoE elements
#[derive(Clone, Copy, Debug, PartialEq, Eq, ethercrab_wire::EtherCrabWireReadWrite)]
#[cfg_attr(test, derive(arbitrary::Arbitrary))]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
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

/// The field near the bottom of SDO definition tables called "Command specifier".
///
/// See e.g. ETG1000.6 Section 5.6.2.6.2 Table 39 – Upload SDO Segment Response.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ethercrab_wire::EtherCrabWireReadWrite)]
#[wire(bits = 3)]
#[repr(u8)]
pub enum CoeCommand {
    DownloadRequest = 0x01,
    UploadRequest = 0x02,
    AbortRequest = 0x04,
    UploadSegmentRequest = 0x03,
}

/// Defined in ETG1000.6 Section 5.6.2.1.1
#[derive(Clone, Copy, Debug, PartialEq, Eq, ethercrab_wire::EtherCrabWireReadWrite)]
#[wire(bytes = 4)]
pub struct InitSdoHeader {
    #[wire(bits = 1)]
    pub size_indicator: bool,
    #[wire(bits = 1)]
    pub expedited_transfer: bool,
    #[wire(bits = 2)]
    pub size: u8,
    #[wire(bits = 1)]
    pub complete_access: bool,
    #[wire(bits = 3)]
    pub command: CoeCommand,
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
    command: CoeCommand,
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
    pub use super::*;
    use ethercrab_wire::{EtherCrabWireRead, EtherCrabWireWriteSized};

    #[test]
    fn sanity_coe_service() {
        assert_eq!(CoeService::SdoRequest.pack(), [0x02]);
        assert_eq!(
            CoeService::unpack_from_slice(&[0x02]),
            Ok(CoeService::SdoRequest)
        );
    }
}
