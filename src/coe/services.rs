use super::{CoeHeader, CoeService, InitSdoFlags, InitSdoHeader, SdoAccess, SegmentSdoHeader};
use crate::{
    error::Error,
    mailbox::{MailboxHeader, MailboxType, Priority},
};
use packed_struct::prelude::PackedStruct;

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

// impl UploadExpeditedRequest {
//     pub fn upload(counter: u8, index: u16, access: SdoAccess) -> Self {
//         Self {
//             header: MailboxHeader {
//                 length: 0x0a,
//                 address: 0x0000,
//                 priority: Priority::Lowest,
//                 mailbox_type: MailboxType::Coe,
//                 counter,
//             },
//             coe_header: CoeHeader {
//                 service: CoeService::SdoRequest,
//             },
//             sdo_header: InitSdoHeader {
//                 flags: InitSdoFlags {
//                     size_indicator: false,
//                     expedited_transfer: false,
//                     size: 0,
//                     complete_access: access.complete_access(),
//                     command: InitSdoFlags::UPLOAD_REQUEST,
//                 },
//                 index,
//                 sub_index: access.sub_index(),
//             },
//         }
//     }
// }

#[derive(Debug, Copy, Clone, PackedStruct)]
pub struct UploadExpeditedResponse {
    #[packed_field(size_bytes = "6")]
    pub header: MailboxHeader,
    #[packed_field(size_bytes = "2")]
    pub coe_header: CoeHeader,
    #[packed_field(size_bytes = "4")]
    pub sdo_header: InitSdoHeader,
    // pub data: [u8; 4],
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

// impl UploadSegmentRequest {
//     pub fn upload_segmented(counter: u8, toggle: bool) -> Self {
//         Self {
//             header: MailboxHeader {
//                 length: 0x0a,
//                 address: 0x0000,
//                 priority: Priority::Lowest,
//                 mailbox_type: MailboxType::Coe,
//                 counter,
//             },
//             coe_header: CoeHeader {
//                 service: CoeService::SdoRequest,
//             },
//             sdo_header: SegmentSdoHeader {
//                 // False/0 when sending
//                 more_follows: false,
//                 segment_data_size: 0,
//                 toggle,
//                 command: SegmentSdoHeader::UPLOAD_SEGMENT_REQUEST,
//             },
//         }
//     }
// }

#[derive(Debug, Copy, Clone)]
pub struct UploadSegmentResponse<const N: usize> {
    pub header: MailboxHeader,
    pub coe_header: CoeHeader,
    pub sdo_header: SegmentSdoHeader,
    // Up to mailbox data size (as defined by slave config) - 3 bytes
    pub data: [u8; N],
}

// --
// --
// --
// --
// --

#[derive(Debug, Copy, Clone, PackedStruct)]
pub struct UploadRequest {
    #[packed_field(size_bytes = "6")]
    pub header: MailboxHeader,
    #[packed_field(size_bytes = "2")]
    pub coe_header: CoeHeader,
    #[packed_field(size_bytes = "4")]
    pub sdo_header: InitSdoHeader,
}

#[derive(Debug, Copy, Clone, PackedStruct)]
pub struct SegmentedUploadRequest {
    #[packed_field(size_bytes = "6")]
    pub header: MailboxHeader,
    #[packed_field(size_bytes = "2")]
    pub coe_header: CoeHeader,
    #[packed_field(size_bytes = "1")]
    pub sdo_header: SegmentSdoHeader,
}

pub trait CoeServiceTrait: packed_struct::PackedStruct {
    fn counter(&self) -> u8;
    fn is_aborted(&self) -> bool;
    fn mailbox_type(&self) -> MailboxType;
}

impl CoeServiceTrait for UploadRequest {
    fn counter(&self) -> u8 {
        self.header.counter
    }
    fn is_aborted(&self) -> bool {
        self.sdo_header.flags.command == InitSdoFlags::ABORT_REQUEST
    }
    fn mailbox_type(&self) -> MailboxType {
        self.header.mailbox_type
    }
}
impl CoeServiceTrait for SegmentedUploadRequest {
    fn counter(&self) -> u8 {
        self.header.counter
    }
    fn is_aborted(&self) -> bool {
        self.sdo_header.command == InitSdoFlags::ABORT_REQUEST
    }
    fn mailbox_type(&self) -> MailboxType {
        self.header.mailbox_type
    }
}

// #[derive(Debug, Copy, Clone, PackedStruct)]
// #[packed_struct(size_bytes = "1", bit_numbering = "lsb0", endian = "lsb")]
// pub struct GenericSdoHeader {

//     #[packed_field(bits = "5..=7")]
//     pub command: u8,
// }

// #[derive(Debug, Copy, Clone, PackedStruct)]
// pub struct GenericResponse {
//     #[packed_field(size_bytes = "6")]
//     pub header: MailboxHeader,
//     #[packed_field(size_bytes = "2")]
//     pub coe_header: CoeHeader,
//     #[packed_field(size_bytes = "1")]
//     pub sdo_header: GenericSdoHeader,
// }

pub fn upload_segmented(counter: u8, toggle: bool) -> SegmentedUploadRequest {
    SegmentedUploadRequest {
        header: MailboxHeader {
            length: 0x0a,
            address: 0x0000,
            priority: Priority::Lowest,
            mailbox_type: MailboxType::Coe,
            counter,
        },
        coe_header: CoeHeader {
            service: CoeService::SdoRequest,
        },
        sdo_header: SegmentSdoHeader {
            // False/0 when sending
            is_last_segment: false,
            segment_data_size: 0,
            toggle,
            command: SegmentSdoHeader::UPLOAD_SEGMENT_REQUEST,
        },
    }
}

pub fn upload(counter: u8, index: u16, access: SdoAccess) -> UploadRequest {
    UploadRequest {
        header: MailboxHeader {
            length: 0x0a,
            address: 0x0000,
            priority: Priority::Lowest,
            mailbox_type: MailboxType::Coe,
            counter,
        },
        coe_header: CoeHeader {
            service: CoeService::SdoRequest,
        },
        sdo_header: InitSdoHeader {
            flags: InitSdoFlags {
                size_indicator: false,
                expedited_transfer: false,
                size: 0,
                complete_access: access.complete_access(),
                command: InitSdoFlags::UPLOAD_REQUEST,
            },
            index,
            sub_index: access.sub_index(),
        },
    }
}
