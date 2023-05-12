use crate::pdu_data::PduStruct;
use core::fmt;
use packed_struct::prelude::*;

/// ETG1000.6 Table 67 – CoE Communication Area, "Sync Manager Communication Type".
pub const SM_TYPE_ADDRESS: u16 = 0x1c00;

/// // ETG1000.6 Table 67 – CoE Communication Area, the address of the first sync manager.
pub const SM_BASE_ADDRESS: u16 = 0x1c10;

/// Sync manager channel.
///
/// Defined in ETG1000.4 6.7.2
#[derive(Default, Copy, Clone, PartialEq, Eq, PackedStruct)]
#[packed_struct(size_bytes = "8", bit_numbering = "msb0", endian = "lsb")]
pub struct SyncManagerChannel {
    #[packed_field(size_bytes = "2")]
    pub physical_start_address: u16,
    #[packed_field(size_bytes = "2")]
    pub length_bytes: u16,
    #[packed_field(size_bytes = "1")]
    pub control: Control,
    #[packed_field(size_bytes = "1")]
    pub status: Status,
    #[packed_field(size_bytes = "2")]
    pub enable: Enable,
}

impl fmt::Debug for SyncManagerChannel {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SyncManagerChannel")
            .field(
                "physical_start_address",
                &format_args!("{:#06x}", self.physical_start_address),
            )
            .field(
                "length_bytes",
                &format_args!("{:#06x} ({})", self.length_bytes, self.length_bytes),
            )
            .field("control", &self.control)
            .field("status", &self.status)
            .field("enable", &self.enable)
            .finish()
    }
}

impl fmt::Display for SyncManagerChannel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_fmt(format_args!(
            "start {:#06x}, size {:#06x} ({}), direction {:?}, mode {:?}, {}",
            self.physical_start_address,
            self.length_bytes,
            self.length_bytes,
            self.control.direction,
            self.control.operation_mode,
            if self.enable.enable {
                "enabled"
            } else {
                "disabled"
            },
        ))
    }
}

impl PduStruct for SyncManagerChannel {}

#[derive(Default, Copy, Clone, Debug, PartialEq, Eq, PackedStruct)]
#[packed_struct(size_bytes = "1", bit_numbering = "lsb0", endian = "lsb")]
pub struct Control {
    #[packed_field(bits = "0..=1", ty = "enum")]
    pub operation_mode: OperationMode,
    #[packed_field(bits = "2..=3", ty = "enum")]
    pub direction: Direction,
    #[packed_field(bits = "4")]
    pub ecat_event_enable: bool,
    #[packed_field(bits = "5")]
    pub dls_user_event_enable: bool,
    #[packed_field(bits = "6")]
    pub watchdog_enable: bool,
    // reserved1: bool
}

#[derive(Default, Copy, Clone, Debug, PartialEq, Eq, PackedStruct)]
#[packed_struct(size_bytes = "1", bit_numbering = "lsb0", endian = "lsb")]
pub struct Status {
    #[packed_field(bits = "0")]
    pub has_write_event: bool,
    #[packed_field(bits = "1")]
    pub has_read_event: bool,
    // reserved1: bool
    #[packed_field(bits = "3")]
    pub mailbox_full: bool,
    #[packed_field(bits = "4..=5", ty = "enum")]
    pub buffer_state: BufferState,
    #[packed_field(bits = "6")]
    pub read_buffer_open: bool,
    #[packed_field(bits = "7")]
    pub write_buffer_open: bool,
}

#[derive(Default, Copy, Clone, Debug, PartialEq, Eq, PackedStruct)]
#[packed_struct(size_bytes = "2", bit_numbering = "lsb0", endian = "lsb")]
pub struct Enable {
    // ---
    // First byte (little endian, so second index)
    // ---
    #[packed_field(bits = "8")]
    pub enable: bool,
    #[packed_field(bits = "9")]
    pub repeat: bool,
    // reserved4: u8
    // TODO: Less insane names
    #[packed_field(bits = "14")]
    pub dc_event0w_busw: bool,
    #[packed_field(bits = "15")]
    pub dc_event0wlocw: bool,
    // ---
    // Second byte (little endian, so first index)
    // ---
    #[packed_field(bits = "0")]
    pub channel_pdi_disabled: bool,
    #[packed_field(bits = "1")]
    pub repeat_ack: bool,
    // #[packed_field(bits = "10..15")]
    // pub _rest: u8,
}

#[derive(Default, Copy, Clone, Debug, PartialEq, Eq, PrimitiveEnum_u8)]
pub enum OperationMode {
    #[default]
    Normal = 0x00,
    Mailbox = 0x02,
}

#[derive(Default, Copy, Clone, Debug, PartialEq, Eq, PrimitiveEnum_u8)]
pub enum Direction {
    #[default]
    MasterRead = 0x00,
    MasterWrite = 0x01,
}

// TODO: More informative names
#[derive(Default, Copy, Clone, Debug, PartialEq, Eq, PrimitiveEnum_u8)]
pub enum BufferState {
    #[default]
    Read = 0x00,
    Second = 0x01,
    Third = 0x02,
    Locked = 0x03,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_zero() {
        assert_eq!(SyncManagerChannel::default().pack().unwrap(), [0u8; 8]);
    }

    #[test]
    fn size() {
        // Packed size
        assert_eq!(SyncManagerChannel::packed_bytes_size(None).unwrap(), 8);
    }

    #[test]
    fn little_endian() {
        // I'm going insane
        assert_eq!(u16::from_le_bytes([0x00, 0x10]), 0x1000);
        assert_eq!(u16::from_le_bytes([0x01, 0x00]), 0x0001);
        assert_eq!(u16::from_le_bytes([0x80, 0x00]), 0x0080);
    }

    #[test]
    fn decode_control() {
        // Fields are little endian
        // Taken from `soem-single-lan9252.pcap`
        let raw = [0x26];

        let parsed = Control::unpack_from_slice(&raw).unwrap();

        assert_eq!(
            parsed,
            Control {
                operation_mode: OperationMode::Mailbox,
                direction: Direction::MasterWrite,
                ecat_event_enable: false,
                dls_user_event_enable: true,
                watchdog_enable: false,
            },
        )
    }

    #[test]
    fn decode_enable() {
        // Fields are little endian
        // Taken from `soem-single-lan9252.pcap`
        let raw = [0x01, 0x00];

        let parsed = Enable::unpack_from_slice(&raw).unwrap();

        assert_eq!(
            parsed,
            Enable {
                enable: true,
                repeat: false,
                dc_event0w_busw: false,
                dc_event0wlocw: false,
                channel_pdi_disabled: false,
                repeat_ack: false,
            }
        )
    }

    #[test]
    fn decode_mailbox_event() {
        let raw = [0x09];

        let parsed = Status::unpack_from_slice(&raw).unwrap();

        assert_eq!(parsed.mailbox_full, true)
    }

    #[test]
    fn encode_enable() {
        let raw = Enable {
            enable: true,
            repeat: false,
            dc_event0w_busw: false,
            dc_event0wlocw: false,
            channel_pdi_disabled: false,
            repeat_ack: false,
        }
        .pack()
        .unwrap();

        assert_eq!(raw, [0x01, 0x00])
    }

    #[test]
    fn decode_one() {
        // Fields are little endian
        // Taken from `soem-single-lan9252.pcap`
        let raw = [
            // Start address
            0x00, 0x10, //
            // Length
            0x80, 0x00, //
            // Control
            0x26, //
            // Status
            0x00, //
            // Enable
            0x01, 0x00,
        ];

        let parsed = SyncManagerChannel::unpack_from_slice(&raw).unwrap();

        assert_eq!(
            parsed,
            SyncManagerChannel {
                physical_start_address: 0x1000,
                length_bytes: 0x0080,
                control: Control {
                    operation_mode: OperationMode::Mailbox,
                    direction: Direction::MasterWrite,
                    ecat_event_enable: false,
                    dls_user_event_enable: true,
                    watchdog_enable: false,
                },
                status: Status {
                    has_write_event: false,
                    has_read_event: false,
                    mailbox_full: false,
                    buffer_state: BufferState::Read,
                    read_buffer_open: false,
                    write_buffer_open: false,
                },
                enable: Enable {
                    enable: true,
                    repeat: false,
                    dc_event0w_busw: false,
                    dc_event0wlocw: false,
                    channel_pdi_disabled: false,
                    repeat_ack: false,
                }
            }
        )
    }
}
