use core::fmt::Display;

use super::{CoeHeader, CoeService, InitSdoFlags, InitSdoHeader, SegmentSdoHeader, SubIndex};
use crate::mailbox::{MailboxHeader, MailboxType, Priority};

/// An expedited (data contained within SDO as opposed to sent in subsequent packets) SDO download
/// request.
#[derive(Debug, Copy, Clone, ethercrab_wire::EtherCatWire)]
#[wire(bytes = 16)]
pub struct SdoExpeditedDownload {
    // #[packed_field(size_bytes = "12")]
    #[wire(bytes = 12)]
    pub headers: SdoNormal,
    #[wire(bytes = 4)]
    pub data: [u8; 4],
}

impl Display for SdoExpeditedDownload {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "SDO expedited({:#06x}:{}",
            self.headers.sdo_header.index, self.headers.sdo_header.sub_index
        )?;

        if self.headers.sdo_header.flags.complete_access {
            write!(f, " complete access)")?;
        } else {
            write!(f, ")")?;
        }

        Ok(())
    }
}

/// A normal SDO request or response with no additional payload.
///
/// These fields are common to non-segmented (i.e. "normal") SDO requests and responses.
///
/// See ETG1000.6 Section 5.6.2 SDO.
#[derive(Debug, Copy, Clone, ethercrab_wire::EtherCatWire)]
#[wire(bytes = 12)]
pub struct SdoNormal {
    // #[packed_field(size_bytes = "6")]
    #[wire(bytes = 6)]
    pub header: MailboxHeader,
    // #[packed_field(size_bytes = "2")]
    #[wire(bytes = 2)]
    pub coe_header: CoeHeader,
    // #[packed_field(size_bytes = "4")]
    #[wire(bytes = 4)]
    pub sdo_header: InitSdoHeader,
}

impl Display for SdoNormal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "SDO normal({:#06x}:{}",
            self.sdo_header.index, self.sdo_header.sub_index
        )?;

        if self.sdo_header.flags.complete_access {
            write!(f, " complete access)")?;
        } else {
            write!(f, ")")?;
        }

        Ok(())
    }
}

/// Headers belonging to segmented SDO transfers.
#[derive(Debug, Copy, Clone, ethercrab_wire::EtherCatWire)]
#[wire(bytes = 9)]
pub struct SdoSegmented {
    #[wire(bytes = 6)]
    // #[packed_field(size_bytes = "6")]
    pub header: MailboxHeader,
    // #[packed_field(size_bytes = "2")]
    #[wire(bytes = 2)]
    pub coe_header: CoeHeader,
    // #[packed_field(size_bytes = "1")]
    #[wire(bytes = 1)]
    pub sdo_header: SegmentSdoHeader,
}

impl Display for SdoSegmented {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "SDO segmented")?;

        Ok(())
    }
}

/// Functionality common to all service responses (normal, expedited, segmented).
pub trait CoeServiceResponse: ethercrab_wire::EtherCatWire {
    fn counter(&self) -> u8;
    fn is_aborted(&self) -> bool;
    fn mailbox_type(&self) -> MailboxType;
    fn address(&self) -> u16;
    fn sub_index(&self) -> u8;
}

/// Must be implemented for any type used to send a CoE service.
pub trait CoeServiceRequest: ethercrab_wire::EtherCatWire {
    type Response: CoeServiceResponse;

    /// Get the auto increment counter value for this request.
    fn counter(&self) -> u8;
}

impl CoeServiceResponse for SdoSegmented {
    /// Get the auto increment counter value for this response.
    fn counter(&self) -> u8 {
        self.header.counter
    }
    fn is_aborted(&self) -> bool {
        self.sdo_header.command == InitSdoFlags::ABORT_REQUEST
    }
    fn mailbox_type(&self) -> MailboxType {
        self.header.mailbox_type
    }
    fn address(&self) -> u16 {
        0
    }
    fn sub_index(&self) -> u8 {
        0
    }
}

impl CoeServiceResponse for SdoNormal {
    fn counter(&self) -> u8 {
        self.header.counter
    }
    fn is_aborted(&self) -> bool {
        self.sdo_header.flags.command == InitSdoFlags::ABORT_REQUEST
    }
    fn mailbox_type(&self) -> MailboxType {
        self.header.mailbox_type
    }
    fn address(&self) -> u16 {
        self.sdo_header.index
    }
    fn sub_index(&self) -> u8 {
        self.sdo_header.sub_index
    }
}

impl CoeServiceRequest for SdoExpeditedDownload {
    type Response = SdoNormal;

    fn counter(&self) -> u8 {
        self.headers.header.counter
    }
}

impl CoeServiceRequest for SdoNormal {
    type Response = Self;

    fn counter(&self) -> u8 {
        self.header.counter
    }
}

impl CoeServiceRequest for SdoSegmented {
    type Response = Self;

    fn counter(&self) -> u8 {
        self.header.counter
    }
}

pub fn download(
    counter: u8,
    index: u16,
    access: SubIndex,
    data: [u8; 4],
    len: u8,
) -> SdoExpeditedDownload {
    SdoExpeditedDownload {
        headers: SdoNormal {
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
        },
        data,
    }
}

pub fn upload_segmented(counter: u8, toggle: bool) -> SdoSegmented {
    SdoSegmented {
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

pub fn upload(counter: u8, index: u16, access: SubIndex) -> SdoNormal {
    SdoNormal {
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
