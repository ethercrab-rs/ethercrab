use super::{CoeService, InitSdoHeader, SdoInfoHeader, SdoInfoOpCode, SegmentSdoHeader, SubIndex};
use crate::mailbox::{MailboxHeader, MailboxType, Priority};
use core::fmt::Display;

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

/// Defined in ETG.1000.6 §5.6.3.3.1
#[derive(Debug, Copy, Clone, PartialEq, ethercrab_wire::EtherCrabWireReadWrite)]
#[wire(bytes = 14)]
pub struct ObjectDescriptionListRequest {
    #[wire(bytes = 8)]
    pub mailbox: MailboxHeader,
    #[wire(bytes = 4)]
    pub sdo_info_header: SdoInfoHeader,
    #[wire(bytes = 2)]
    pub list_type: ObjectDescriptionListQueryInner,
}

/// Defined in ETG.1000.6 §5.6.3.3.2
#[derive(Debug, Copy, Clone, PartialEq, ethercrab_wire::EtherCrabWireReadWrite)]
#[wire(bytes = 12)]
pub struct ObjectDescriptionListResponse {
    #[wire(bytes = 8)]
    pub mailbox: MailboxHeader,
    #[wire(bytes = 4)]
    pub sdo_info_header: SdoInfoHeader,
}

/// [`ObjectDescriptionListQuery`], but with `ObjectQuantities`.
#[derive(Debug, Copy, Clone, PartialEq, ethercrab_wire::EtherCrabWireReadWrite)]
#[repr(u8)]
pub enum ObjectDescriptionListQueryInner {
    /// Get number of objects in the 5 different lists.
    ObjectQuantities = 0x00,
    /// All objects of the object dictionary.
    All = 0x01,
    /// Objects which are mappable in an RxPDO.
    RxPdoMappable = 0x02,
    /// Objects which are mappable in a TxPDO.
    TxPdoMappable = 0x03,
    /// Objects which have to be stored for a device replacement.
    StoredForDeviceReplacement = 0x04,
    /// Objects which can be used as startup parameter.
    StartupParameters = 0x05,
}

/// The subset of indices of the object dictionary which
/// [`crate::SubDeviceRef::sdo_info_object_description_list`] makes a request for.
///
/// Defined in ETG.1000.6 §5.6.3.3.1.
///
/// Note that object quantities (value 0 in the standard) can be queried with
/// [`crate::SubDeviceRef::sdo_info_object_quantities`].
#[derive(Debug, Copy, Clone, ethercrab_wire::EtherCrabWireReadWrite)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[repr(u8)]
pub enum ObjectDescriptionListQuery {
    // ObjectQuantities is invoked through a different API
    /// All objects of the object dictionary.
    All = 0x01,
    /// Objects which are mappable in an RxPDO.
    RxPdoMappable = 0x02,
    /// Objects which are mappable in a TxPDO.
    TxPdoMappable = 0x03,
    /// Objects which have to be stored for a device replacement.
    StoredForDeviceReplacement = 0x04,
    /// Objects which can be used as startup parameter.
    StartupParameters = 0x05,
}

impl From<ObjectDescriptionListQuery> for ObjectDescriptionListQueryInner {
    fn from(user: ObjectDescriptionListQuery) -> Self {
        match user {
            ObjectDescriptionListQuery::All => ObjectDescriptionListQueryInner::All,
            ObjectDescriptionListQuery::RxPdoMappable => {
                ObjectDescriptionListQueryInner::RxPdoMappable
            }
            ObjectDescriptionListQuery::TxPdoMappable => {
                ObjectDescriptionListQueryInner::TxPdoMappable
            }
            ObjectDescriptionListQuery::StoredForDeviceReplacement => {
                ObjectDescriptionListQueryInner::StoredForDeviceReplacement
            }
            ObjectDescriptionListQuery::StartupParameters => {
                ObjectDescriptionListQueryInner::StartupParameters
            }
        }
    }
}

/// How many CoE objects on a subdevice are of each [`ObjectDescriptionListQuery`].
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct ObjectDescriptionListQueryCounts {
    /// How many are of type [`ObjectDescriptionListQuery::All`].
    pub all: u16,
    /// How many are of type [`ObjectDescriptionListQuery::RxPdoMappable`].
    pub rx_pdo_mappable: u16,
    /// How many are of type [`ObjectDescriptionListQuery::TxPdoMappable`].
    pub tx_pdo_mappable: u16,
    /// How many are of type [`ObjectDescriptionListQuery::StoredForDeviceReplacement`].
    pub stored_for_device_replacement: u16,
    /// How many are of type [`ObjectDescriptionListQuery::StartupParameters`].
    pub startup_parameters: u16,
}

impl core::fmt::Display for ObjectDescriptionListQuery {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                ObjectDescriptionListQuery::All => "All",
                ObjectDescriptionListQuery::RxPdoMappable => "RxPDO-Mappable",
                ObjectDescriptionListQuery::TxPdoMappable => "TxPDO-Mappable",
                ObjectDescriptionListQuery::StoredForDeviceReplacement =>
                    "Stored for Device Replacement",
                ObjectDescriptionListQuery::StartupParameters => "Startup Parameters",
            }
        )
    }
}

/// Must be implemented for any type used to send a CoE SDO Request or Response service.
pub trait CoeServiceRequest:
    ethercrab_wire::EtherCrabWireReadWrite + ethercrab_wire::EtherCrabWireWriteSized
{
    fn validate_response(&self, received_index: u16, received_subindex: u8) -> bool;
}

impl CoeServiceRequest for SdoExpeditedDownload {
    fn validate_response(&self, received_index: u16, received_subindex: u8) -> bool {
        received_index == self.headers.sdo_header.index
            && received_subindex == self.headers.sdo_header.sub_index
    }
}

impl CoeServiceRequest for SdoNormal {
    fn validate_response(&self, received_index: u16, received_subindex: u8) -> bool {
        received_index == self.sdo_header.index && received_subindex == self.sdo_header.sub_index
    }
}

impl CoeServiceRequest for SdoSegmented {
    // No values to check against, so always valid
    fn validate_response(&self, _received_index: u16, _received_subindex: u8) -> bool {
        true
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
                // address: 0x0000,
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
                command: super::CoeCommand::Download,
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
            // address: 0x0000,
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
            command: super::CoeCommand::UploadSegment,
        },
    }
}

pub fn upload(counter: u8, index: u16, access: SubIndex) -> SdoNormal {
    SdoNormal {
        header: MailboxHeader {
            length: 0x0a,
            // address: 0x0000,
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
            command: super::CoeCommand::Upload,
            index,
            sub_index: access.sub_index(),
        },
    }
}

pub fn get_object_description_list(
    counter: u8,
    list_type: ObjectDescriptionListQuery,
) -> ObjectDescriptionListRequest {
    ObjectDescriptionListRequest {
        mailbox: MailboxHeader {
            length: 0x08,
            // address: 0x0000,
            priority: Priority::Lowest,
            mailbox_type: MailboxType::Coe,
            counter,
            service: CoeService::SdoInformation,
        },
        sdo_info_header: SdoInfoHeader {
            op_code: SdoInfoOpCode::GetObjectDescriptionListRequest,
            incomplete: false,
            fragments_left: 0,
        },
        list_type: list_type.into(),
    }
}

pub fn get_object_quantities(counter: u8) -> ObjectDescriptionListRequest {
    ObjectDescriptionListRequest {
        mailbox: MailboxHeader {
            length: 0x08,
            // address: 0x0000,
            priority: Priority::Lowest,
            mailbox_type: MailboxType::Coe,
            counter,
            service: CoeService::SdoInformation,
        },
        sdo_info_header: SdoInfoHeader {
            op_code: SdoInfoOpCode::GetObjectDescriptionListRequest,
            incomplete: false,
            fragments_left: 0,
        },
        list_type: ObjectDescriptionListQueryInner::ObjectQuantities,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::CoeAbortCode;
    use ethercrab_wire::{EtherCrabWireRead, EtherCrabWireSized, EtherCrabWireWrite};

    #[test]
    fn decode_sdo_response_normal() {
        let raw = [10u8, 0, 0, 0, 0, 83, 0, 48, 79, 0, 28, 4];

        let expected = SdoNormal {
            header: MailboxHeader {
                length: 10,
                // address: 0,
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
                command: crate::coe::CoeCommand::Upload,
                index: 0x1c00,
                sub_index: 4,
            },
        };

        assert_eq!(SdoNormal::unpack_from_slice(&raw), Ok(expected));
    }

    #[test]
    fn encode_sdo_request() {
        let buf = [0xaau8, 0xbb, 0xcc, 0xdd];

        let request = download(123, 0x1234, 3.into(), buf, buf.packed_len() as u8);

        pretty_assertions::assert_eq!(
            request,
            SdoExpeditedDownload {
                headers: SdoNormal {
                    header: MailboxHeader {
                        length: 10,
                        // address: 0,
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
                        command: crate::coe::CoeCommand::Download,
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
                        // address: 0,
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
                        command: crate::coe::CoeCommand::Download,
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
                    // address: 0,
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
                    command: crate::coe::CoeCommand::Upload,
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
                // address: 0,
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
                command: crate::coe::CoeCommand::Upload,
                index: 0x1008,
                sub_index: 0,
            },
        };

        pretty_assertions::assert_eq!(
            SdoNormal::unpack_from_slice(&raw[0..12]),
            Ok(expected_headers)
        );

        assert_eq!(&raw[(12 + u32::PACKED_LEN)..][..4], &[69, 75, 49, 57]);
    }

    #[test]
    fn get_object_description_list() {
        // from Wireshark
        let raw: [u8; 14] = [
            0x8, 0x0, 0x0, 0x0, 0x0, 0x73, 0x0, 0x80, 0x1, 0x0, 0x0, 0x0, 0x1, 0x0,
        ];
        let parsed = ObjectDescriptionListRequest::unpack_from_slice(&raw);
        let expected = ObjectDescriptionListRequest {
            mailbox: MailboxHeader {
                length: 8,
                priority: Priority::Lowest,
                mailbox_type: MailboxType::Coe,
                counter: 7,
                service: CoeService::SdoInformation,
            },
            sdo_info_header: SdoInfoHeader {
                op_code: SdoInfoOpCode::GetObjectDescriptionListRequest,
                incomplete: false,
                fragments_left: 0,
            },
            list_type: ObjectDescriptionListQueryInner::All,
        };
        pretty_assertions::assert_eq!(parsed, Ok(expected));
        let mut buf = [0u8; ObjectDescriptionListRequest::PACKED_LEN];
        pretty_assertions::assert_eq!(
            expected.pack_to_slice(&mut buf),
            Ok(&raw[..ObjectDescriptionListRequest::PACKED_LEN])
        );

        // from Wireshark
        const RAW_LEN: usize = 128;
        let raw: [u8; RAW_LEN] = [
            0x7a, 0x0, 0x0, 0x0, 0x0, 0x73, 0x0, 0x80, 0x82, 0x0, 0x1, 0x0, 0x1, 0x0, 0x0, 0x10,
            0x8, 0x10, 0x9, 0x10, 0xa, 0x10, 0x18, 0x10, 0x1, 0x16, 0x2, 0x16, 0x3, 0x16, 0x21,
            0x16, 0x22, 0x16, 0x23, 0x16, 0x24, 0x16, 0x25, 0x16, 0x26, 0x16, 0x30, 0x16, 0x31,
            0x16, 0x0, 0x1a, 0x1, 0x1a, 0x2, 0x1a, 0x3, 0x1a, 0x4, 0x1a, 0x5, 0x1a, 0x6, 0x1a, 0x7,
            0x1a, 0x8, 0x1a, 0x9, 0x1a, 0xa, 0x1a, 0xb, 0x1a, 0xc, 0x1a, 0xd, 0x1a, 0xe, 0x1a, 0xf,
            0x1a, 0x10, 0x1a, 0x11, 0x1a, 0x12, 0x1a, 0x13, 0x1a, 0x14, 0x1a, 0x15, 0x1a, 0x16,
            0x1a, 0x17, 0x1a, 0x18, 0x1a, 0x19, 0x1a, 0x1a, 0x1a, 0x1b, 0x1a, 0x1c, 0x1a, 0x1d,
            0x1a, 0x1e, 0x1a, 0x1f, 0x1a, 0x20, 0x1a, 0x21, 0x1a, 0x22, 0x1a, 0x23, 0x1a, 0x24,
            0x1a, 0x25, 0x1a, 0x26, 0x1a, 0x30, 0x1a, 0x31, 0x1a,
        ];
        let parsed = ObjectDescriptionListResponse::unpack_from_slice(&raw);
        let expected = ObjectDescriptionListResponse {
            mailbox: MailboxHeader {
                length: 122,
                priority: Priority::Lowest,
                mailbox_type: MailboxType::Coe,
                counter: 7,
                service: CoeService::SdoInformation,
            },
            sdo_info_header: SdoInfoHeader {
                op_code: SdoInfoOpCode::GetObjectDescriptionListResponse,
                incomplete: true,
                fragments_left: 1,
            },
        };
        pretty_assertions::assert_eq!(parsed, Ok(expected));
        let list_type = <ObjectDescriptionListQueryInner>::unpack_from_slice(
            &raw[ObjectDescriptionListResponse::PACKED_LEN
                ..ObjectDescriptionListResponse::PACKED_LEN + 2],
        );
        pretty_assertions::assert_eq!(list_type, Ok(ObjectDescriptionListQueryInner::All));
        let mut buf = [0u8; ObjectDescriptionListResponse::PACKED_LEN];
        pretty_assertions::assert_eq!(
            expected.pack_to_slice(&mut buf),
            Ok(&raw[..ObjectDescriptionListResponse::PACKED_LEN])
        );
        // length is actually 57
        let expected: [u16; (RAW_LEN - ObjectDescriptionListRequest::PACKED_LEN) / 2] = [
            0x1000, 0x1008, 0x1009, 0x100a, 0x1018, 0x1601, 0x1602, 0x1603, 0x1621, 0x1622, 0x1623,
            0x1624, 0x1625, 0x1626, 0x1630, 0x1631, 0x1a00, 0x1a01, 0x1a02, 0x1a03, 0x1a04, 0x1a05,
            0x1a06, 0x1a07, 0x1a08, 0x1a09, 0x1a0a, 0x1a0b, 0x1a0c, 0x1a0d, 0x1a0e, 0x1a0f, 0x1a10,
            0x1a11, 0x1a12, 0x1a13, 0x1a14, 0x1a15, 0x1a16, 0x1a17, 0x1a18, 0x1a19, 0x1a1a, 0x1a1b,
            0x1a1c, 0x1a1d, 0x1a1e, 0x1a1f, 0x1a20, 0x1a21, 0x1a22, 0x1a23, 0x1a24, 0x1a25, 0x1a26,
            0x1a30, 0x1a31,
        ];
        let parsed = <_>::unpack_from_slice(&raw[ObjectDescriptionListRequest::PACKED_LEN..]);
        pretty_assertions::assert_eq!(parsed, Ok(expected));
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
                // address: 0x0000,
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
                command: crate::coe::CoeCommand::Abort,
                index: 0x1001,
                sub_index: 0,
            },
        };

        let abort_code = CoeAbortCode::unpack_from_slice(&raw[12..]);

        assert_eq!(abort_code, Ok(CoeAbortCode::NotFound));

        pretty_assertions::assert_eq!(parsed, Ok(expected));
    }
}
