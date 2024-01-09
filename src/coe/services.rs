use super::{CoeService, InitSdoHeader, SegmentSdoHeader, SubIndex};
use crate::mailbox::{MailboxHeader, MailboxType, Priority};
use core::fmt::Display;
use ethercrab_wire::EtherCrabWireSized;

/// An expedited (data contained within SDO as opposed to sent in subsequent packets) SDO download
/// request.
#[derive(Debug, Copy, Clone, PartialEq, ethercrab_wire::EtherCrabWireReadWrite)]
#[wire(bytes = 16)]
pub struct SdoExpeditedDownload {
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

        if self.headers.sdo_header.complete_access {
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
#[derive(Debug, Copy, Clone, PartialEq, ethercrab_wire::EtherCrabWireReadWrite)]
#[wire(bytes = 12)]
pub struct SdoNormal {
    #[wire(bytes = 8)]
    pub header: MailboxHeader,
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

        if self.sdo_header.complete_access {
            write!(f, " complete access)")?;
        } else {
            write!(f, ")")?;
        }

        Ok(())
    }
}

/// Headers belonging to segmented SDO transfers.
#[derive(Debug, Copy, Clone, ethercrab_wire::EtherCrabWireReadWrite)]
#[wire(bytes = 9)]
pub struct SdoSegmented {
    #[wire(bytes = 8)]
    pub header: MailboxHeader,
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
pub trait CoeServiceResponse {
    fn counter(&self) -> u8;
    fn is_aborted(&self) -> bool;
    fn mailbox_type(&self) -> MailboxType;
    fn address(&self) -> u16;
    fn sub_index(&self) -> u8;

    fn header_len() -> usize;
}

/// Must be implemented for any type used to send a CoE service.
pub trait CoeServiceRequest: ethercrab_wire::EtherCrabWireWrite {
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
        self.sdo_header.command == InitSdoHeader::ABORT_REQUEST
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
    fn header_len() -> usize {
        Self::PACKED_LEN
    }
}

impl CoeServiceResponse for SdoNormal {
    fn counter(&self) -> u8 {
        self.header.counter
    }
    fn is_aborted(&self) -> bool {
        self.sdo_header.command == InitSdoHeader::ABORT_REQUEST
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
    fn header_len() -> usize {
        Self::PACKED_LEN
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
                service: CoeService::SdoRequest,
            },
            sdo_header: InitSdoHeader {
                size_indicator: true,
                expedited_transfer: true,
                size: 4u8.saturating_sub(len),
                complete_access: access.complete_access(),
                command: InitSdoHeader::DOWNLOAD_REQUEST,
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
            service: CoeService::SdoRequest,
        },
        sdo_header: InitSdoHeader {
            size_indicator: false,
            expedited_transfer: false,
            size: 0,
            complete_access: access.complete_access(),
            command: InitSdoHeader::UPLOAD_REQUEST,
            index,
            sub_index: access.sub_index(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::CoeAbortCode;
    use ethercrab_wire::{EtherCrabWireRead, EtherCrabWireWrite};

    #[test]
    fn decode_sdo_response_normal() {
        let raw = [10u8, 0, 0, 0, 0, 83, 0, 48, 79, 0, 28, 4];

        let expected = SdoNormal {
            header: MailboxHeader {
                length: 10,
                address: 0,
                priority: Priority::Lowest,
                mailbox_type: MailboxType::Coe,
                counter: 5,
                service: CoeService::SdoResponse,
            },
            sdo_header: InitSdoHeader {
                size_indicator: true,
                expedited_transfer: true,
                size: 3,
                complete_access: false,
                command: 2,
                index: 0x1c00,
                sub_index: 4,
            },
        };

        assert_eq!(CoeServiceResponse::counter(&expected), 5);
        assert_eq!(expected.is_aborted(), false);
        assert_eq!(expected.mailbox_type(), MailboxType::Coe);
        assert_eq!(expected.address(), 0x1c00);
        assert_eq!(expected.sub_index(), 4);

        assert_eq!(SdoNormal::unpack_from_slice(&raw), Ok(expected));
    }

    #[test]
    fn encode_sdo_request() {
        let buf = [0xaau8, 0xbb, 0xcc, 0xdd];

        let request = download(123, 0x1234, 3.into(), buf.clone(), buf.packed_len() as u8);

        pretty_assertions::assert_eq!(
            request,
            SdoExpeditedDownload {
                headers: SdoNormal {
                    header: MailboxHeader {
                        length: 10,
                        address: 0,
                        priority: Priority::Lowest,
                        mailbox_type: MailboxType::Coe,
                        counter: 123,
                        service: CoeService::SdoRequest,
                    },
                    sdo_header: InitSdoHeader {
                        size_indicator: true,
                        expedited_transfer: true,
                        size: 0,
                        complete_access: false,
                        command: 1,
                        index: 0x1234,
                        sub_index: 3,
                    },
                },
                data: buf
            }
        )
    }

    #[test]
    fn encode_sdo_request_complete() {
        let buf = [0xaau8, 0xbb, 0xcc, 0xdd];

        let request = download(123, 0x1234, SubIndex::Complete, buf, buf.packed_len() as u8);

        pretty_assertions::assert_eq!(
            request,
            SdoExpeditedDownload {
                headers: SdoNormal {
                    header: MailboxHeader {
                        length: 10,
                        address: 0,
                        priority: Priority::Lowest,
                        mailbox_type: MailboxType::Coe,
                        counter: 123,
                        service: CoeService::SdoRequest,
                    },
                    sdo_header: InitSdoHeader {
                        size_indicator: true,
                        expedited_transfer: true,
                        size: 0,
                        complete_access: true,
                        command: 1,
                        index: 0x1234,
                        // MUST be 1 if complete access is used
                        sub_index: 1,
                    },
                },
                data: buf
            }
        )
    }

    #[test]
    fn upload_request_normal() {
        let request = upload(210, 0x4567, 2.into());

        pretty_assertions::assert_eq!(
            request,
            SdoNormal {
                header: MailboxHeader {
                    length: 10,
                    address: 0,
                    priority: Priority::Lowest,
                    mailbox_type: MailboxType::Coe,
                    counter: 210,
                    service: CoeService::SdoRequest,
                },
                sdo_header: InitSdoHeader {
                    size_indicator: false,
                    expedited_transfer: false,
                    size: 0,
                    complete_access: false,
                    command: 2,
                    index: 0x4567,
                    sub_index: 2,
                },
            }
        )
    }

    #[test]
    fn upload_request_response_segmented() {
        let raw = [
            16u8, 0, 0, 0, 0, 99, 0, 48, 65, 8, 16, 0, 6, 0, 0, 0, 69, 75, 49, 57, 49, 52, 68, 105,
            97, 103, 110, 111, 115, 101, 32, 77, 67, 50, 0, 86, 111, 108, 116, 97, 103, 101, 116,
            97, 103, 101, 115, 105, 118, 101, 1, 112, 16, 112, 0, 128, 1, 128, 2, 128, 14, 128, 1,
            144,
        ];

        let expected_headers = SdoNormal {
            header: MailboxHeader {
                length: 16,
                address: 0,
                priority: Priority::Lowest,
                mailbox_type: MailboxType::Coe,
                counter: 6,
                service: CoeService::SdoResponse,
            },
            sdo_header: InitSdoHeader {
                size_indicator: true,
                expedited_transfer: false,
                size: 0,
                complete_access: false,
                command: 2,
                index: 0x1008,
                sub_index: 0,
            },
        };

        assert_eq!(CoeServiceResponse::counter(&expected_headers), 6);
        assert_eq!(expected_headers.is_aborted(), false);
        assert_eq!(expected_headers.mailbox_type(), MailboxType::Coe);
        assert_eq!(expected_headers.address(), 0x1008);
        assert_eq!(expected_headers.sub_index(), 0);

        pretty_assertions::assert_eq!(
            SdoNormal::unpack_from_slice(&raw[0..12]),
            Ok(expected_headers)
        );

        assert_eq!(&raw[(12 + u32::PACKED_LEN)..][..4], &[69, 75, 49, 57]);
    }

    #[test]
    fn error_not_found() {
        // Copypasta'd from Wireshark
        let raw = [
            0x0a, 0x00, 0x00, 0x00, 0x00, 0x63, 0x00, 0x20, 0x80, 0x01, 0x10, 0x00, 0x00, 0x00,
            0x02, 0x06,
        ];

        let parsed = SdoNormal::unpack_from_slice(&raw);

        let expected = SdoNormal {
            header: MailboxHeader {
                length: 0x0a,
                address: 0x0000,
                priority: Priority::Lowest,
                mailbox_type: MailboxType::Coe,
                counter: 6,
                service: CoeService::SdoRequest,
            },
            sdo_header: InitSdoHeader {
                size_indicator: false,
                expedited_transfer: false,
                size: 0,
                complete_access: false,
                command: InitSdoHeader::ABORT_REQUEST,
                index: 0x1001,
                sub_index: 0,
            },
        };

        let abort_code = CoeAbortCode::unpack_from_slice(&raw[SdoNormal::header_len()..]);

        assert_eq!(abort_code, Ok(CoeAbortCode::NotFound));

        pretty_assertions::assert_eq!(parsed, Ok(expected));
    }
}
