pub mod services;

use core::num::NonZeroU8;

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

    fn unpack(src: &Self::ByteArray) -> packed_struct::PackingResult<Self> {
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
    pub const DOWNLOAD_RESPONSE: u8 = 0x03;
    pub const UPLOAD_REQUEST: u8 = 0x02;
    pub const UPLOAD_RESPONSE: u8 = 0x02;
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
#[packed_struct(size_bytes = "1", bit_numbering = "msb0", endian = "lsb")]
pub struct SegmentSdoHeader {
    #[packed_field(size_bits = "1")]
    pub more_follows: bool,
    #[packed_field(size_bits = "3")]
    /// Segment data size, `0x00` to `0x07`.
    pub segment_data_size: u8,
    #[packed_field(size_bits = "1")]
    pub toggle: bool,
    #[packed_field(size_bits = "3")]
    command: u8,
}

impl SegmentSdoHeader {
    const DOWNLOAD_SEGMENT_REQUEST: u8 = 0x00;
    const DOWNLOAD_SEGMENT_RESPONSE: u8 = 0x01;
    const UPLOAD_SEGMENT_RESPONSE: u8 = 0x02;
    const UPLOAD_SEGMENT_REQUEST: u8 = 0x03;
}

pub enum SdoAccess {
    /// Complete access.
    Complete,

    /// Individual sub-index access.
    Index(u8),
}

impl SdoAccess {
    pub(crate) fn complete_access(&self) -> bool {
        matches!(self, Self::Complete)
    }

    pub(crate) fn sub_index(&self) -> u8 {
        match self {
            SdoAccess::Complete => 0,
            SdoAccess::Index(idx) => *idx,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{services::DownloadResponse, *};
    use crate::{
        coe::{services::UploadSegmentResponse, CoeService, InitSdoFlags, InitSdoHeader},
        mailbox::{MailboxHeader, MailboxType, Priority},
    };
    use chrono::Utc;
    use pcap::{Capture, Linktype, Packet, PacketHeader};
    use std::path::PathBuf;

    #[test]
    fn pack_coe_header() {
        let header = CoeHeader {
            service: CoeService::SdoRequest,
        };

        let packed = header.pack().unwrap();

        assert_eq!(packed, [0x00, 0x20]);
    }

    fn write_bytes_to_file(name: &str, data: &[u8]) {
        let mut frame = crate::pdu_loop::pdu_frame::Frame::default();

        frame
            .replace(
                crate::command::Command::Fpwr {
                    address: 0x1001,
                    register: 0x1800,
                },
                data.len() as u16,
                0xaa,
            )
            .unwrap();

        let mut buffer = Vec::with_capacity(1536);
        buffer.resize(1536, 0);

        frame
            .to_ethernet_frame(buffer.as_mut_slice(), data)
            .unwrap();

        // Epic haxx: force length header param to 1024. This should be the mailbox buffer size
        buffer.as_mut_slice()[0x16] = 0x00;
        buffer.as_mut_slice()[0x17] = 0x04;

        let packet = Packet {
            header: &PacketHeader {
                ts: libc::timeval {
                    tv_sec: Utc::now().timestamp().try_into().expect("Time overflow"),
                    tv_usec: 0,
                },
                // 64 bytes minimum frame size, minus 2x MAC address and 1x optional tag
                caplen: (buffer.len() as u32).max(46),
                len: buffer.len() as u32,
            },
            data: &buffer,
        };

        let cap = Capture::dead(Linktype::ETHERNET).expect("Open capture");

        let path = PathBuf::from(&name);

        let mut save = cap.savefile(&path).expect("Open save file");

        save.write(&packet);
        drop(save);
    }

    // #[test]
    // fn decode_mailbox_upload_response() {
    //     // From Wireshark capture "soem-single-lan9252.pcapng", packet #301
    //     let raw = [
    //         0x0au8, 0x00, 0x00, 0x00, 0x00, 0x13, 0x00, 0x20, 0x50, 0x00, 0x1c, 0x00, 0x00, 0x00,
    //         0x00, 0x00, //
    //         // Data
    //         0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    //     ];

    //     let expected = UploadSegmentResponse::<8> {
    //         header: MailboxHeader {
    //             length: 10,
    //             address: 0x0000,
    //             priority: Priority::Lowest,
    //             mailbox_type: MailboxType::Coe,
    //             counter: 1,
    //         },
    //         coe_header: CoeHeader {
    //             number: 0,
    //             service: CoeService::SdoRequest,
    //         },
    //         sdo_header: SegmentSdoHeader {
    //             more_follows: false,
    //             segment_data_size: 0x00,
    //             toggle: false,
    //             command: SegmentSdoHeader::UPLOAD_SEGMENT_RESPONSE,
    //         },
    //         data: [0u8; 8],
    //     };

    //     let parsed = UploadSegmentResponse::unpack(&raw).unwrap();

    //     assert_eq!(parsed, expected);
    // }

    // #[test]
    // fn encode_mailbox_frame() {
    //     let mbox = DownloadResponse {
    //         header: MailboxHeader {
    //             length: 10,
    //             address: 0x0000,
    //             priority: Priority::Lowest,
    //             mailbox_type: MailboxType::Coe,
    //             counter: 2,
    //         },
    //         coe_header: CoeHeader {
    //             number: 0,
    //             service: CoeService::SdoResponse,
    //         },
    //         sdo_header: InitSdoHeader {
    //             flags: InitSdoFlags {
    //                 size_indicator: false,
    //                 expedited_transfer: false,
    //                 size: 0,
    //                 complete_access: false,
    //                 command: InitSdoFlags::DOWNLOAD_RESPONSE,
    //             },
    //             index: 0x1c12,
    //             sub_index: 0,
    //         },
    //     };

    //     let packed = mbox.pack().unwrap();

    //     write_bytes_to_file("mailbox-encode_mailbox_frame.pcapng", &packed);
    // }
}
