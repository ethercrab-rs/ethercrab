use super::{CoeHeader, CoeService, InitSdoFlags, InitSdoHeader, SdoAccess, SegmentSdoHeader};
use crate::mailbox::{MailboxHeader, MailboxType, Priority};
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
impl CoeServiceTrait for DownloadExpeditedRequest {
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

pub fn download(
    counter: u8,
    index: u16,
    access: SdoAccess,
    data: [u8; 4],
    len: u8,
) -> DownloadExpeditedRequest {
    DownloadExpeditedRequest {
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
                size_indicator: true,
                expedited_transfer: true,
                size: 4u8.saturating_sub(len),
                complete_access: access.complete_access(),
                command: InitSdoFlags::DOWNLOAD_REQUEST,
            },
            index,
            sub_index: access.sub_index(),
        },
        data,
    }
}

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
