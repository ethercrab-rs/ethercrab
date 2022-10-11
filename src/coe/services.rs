use packed_struct::prelude::PackedStruct;

use super::{CoeHeader, InitSdoHeader, SegmentSdoHeader};
use crate::mailbox::MailboxHeader;

#[derive(Debug, Copy, Clone, PackedStruct)]
pub struct DownloadExpeditedRequest {
    #[packed_field(size_bytes = "6")]
    pub header: MailboxHeader,
    #[packed_field(size_bytes = "2")]
    pub coe_header: CoeHeader,
    #[packed_field(size_bytes = "4")]
    pub sdo_header: InitSdoHeader,
    pub data: [u8; 4],
}

// Used for both expedited and normal downloads
#[derive(Debug, Copy, Clone)]
pub struct DownloadResponse {
    pub header: MailboxHeader,
    pub coe_header: CoeHeader,
    pub sdo_header: InitSdoHeader,
    // _reserved: u32
}

#[derive(Debug, Copy, Clone)]
pub struct DownloadNormalRequest<const N: usize> {
    pub header: MailboxHeader,
    pub coe_header: CoeHeader,
    pub sdo_header: InitSdoHeader,
    // The total size of the request in bytes
    pub complete_size: u32,
    // Up to mailbox data size (as defined by slave config) - 10 bytes
    pub data: [u8; N],
}

#[derive(Debug, Copy, Clone)]
pub struct DownloadSegmentRequest<const N: usize> {
    pub header: MailboxHeader,
    pub coe_header: CoeHeader,
    pub sdo_header: SegmentSdoHeader,
    // Up to mailbox data size (as defined by slave config) - 3 bytes
    pub data: [u8; N],
}

#[derive(Debug, Copy, Clone)]
pub struct DownloadSegmentResponse {
    pub header: MailboxHeader,
    pub coe_header: CoeHeader,
    pub sdo_header: SegmentSdoHeader,
    pub data: [u8; 7],
}

#[derive(Debug, Copy, Clone, PackedStruct)]
pub struct UploadExpeditedRequest {
    #[packed_field(size_bytes = "6")]
    pub header: MailboxHeader,
    #[packed_field(size_bytes = "2")]
    pub coe_header: CoeHeader,
    #[packed_field(size_bytes = "4")]
    pub sdo_header: InitSdoHeader,
    // _reserved: u32
}

#[derive(Debug, Copy, Clone, PackedStruct)]
pub struct UploadExpeditedResponse {
    #[packed_field(size_bytes = "6")]
    pub header: MailboxHeader,
    #[packed_field(size_bytes = "2")]
    pub coe_header: CoeHeader,
    #[packed_field(size_bytes = "4")]
    pub sdo_header: InitSdoHeader,
    pub data: [u8; 4],
}

#[derive(Debug, Copy, Clone)]
pub struct UploadNormalResponse<const N: usize> {
    pub header: MailboxHeader,
    pub coe_header: CoeHeader,
    pub sdo_header: InitSdoHeader,
    pub complete_size: u32,
    // Up to mailbox data size (as defined by slave config) - 10 bytes
    pub data: [u8; N],
}

#[derive(Debug, Copy, Clone)]
pub struct UploadSegmentRequest {
    pub header: MailboxHeader,
    pub coe_header: CoeHeader,
    pub sdo_header: SegmentSdoHeader,
}

#[derive(Debug, Copy, Clone)]
pub struct UploadSegmentResponse<const N: usize> {
    pub header: MailboxHeader,
    pub coe_header: CoeHeader,
    pub sdo_header: SegmentSdoHeader,
    // Up to mailbox data size (as defined by slave config) - 3 bytes
    pub data: [u8; N],
}
