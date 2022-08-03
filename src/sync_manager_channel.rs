use packed_struct::prelude::*;

/// Sync manager channel.
///
/// Defined in ETG1000.4 6.7.2
#[derive(Default, Copy, Clone, Debug, PartialEq, PackedStruct)]
#[packed_struct(size_bytes = "8", bit_numbering = "msb0", endian = "lsb")]
pub struct SyncManagerChannel {
    #[packed_field(bits = "0..=15")]
    pub physical_start_address: u16,
    #[packed_field(bits = "16..=31")]
    pub length: u16,
    #[packed_field(bits = "32..=47", element_size_bytes = "2")]
    pub control: Control,
    #[packed_field(bits = "48..=63", element_size_bytes = "2")]
    pub enable: Enable,
}

#[derive(Default, Copy, Clone, Debug, PartialEq, PackedStruct)]
#[packed_struct(size_bytes = "2", bit_numbering = "lsb0", endian = "lsb")]
pub struct Control {
    // ---
    // First byte (little endian, so second index)
    // ---
    #[packed_field(bits = "8..=9", ty = "enum")]
    pub operation_mode: OperationMode,
    #[packed_field(bits = "10..=11", ty = "enum")]
    pub direction: Direction,
    #[packed_field(bits = "12")]
    pub ecat_event_enable: bool,
    #[packed_field(bits = "13")]
    pub dls_user_event_enable: bool,
    #[packed_field(bits = "14")]
    pub watchdog_enable: bool,
    // reserved1: bool
    // ---
    // Second byte (little endian, so first index)
    // ---
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

#[derive(Default, Copy, Clone, Debug, PartialEq, PackedStruct)]
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

#[derive(Default, Copy, Clone, Debug, PartialEq, PrimitiveEnum_u8)]
pub enum OperationMode {
    #[default]
    Buffered = 0x00,
    Mailbox = 0x02,
}

#[derive(Default, Copy, Clone, Debug, PartialEq, PrimitiveEnum_u8)]
pub enum Direction {
    #[default]
    MasterRead = 0x00,
    MasterWrite = 0x01,
}

// TODO: More informative names
#[derive(Default, Copy, Clone, Debug, PartialEq, PrimitiveEnum_u8)]
pub enum BufferState {
    #[default]
    First = 0x00,
    Second = 0x01,
    Third = 0x02,
    Locked = 0x03,
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let raw = [0x26, 0x00];

        let parsed = Control::unpack_from_slice(&raw).unwrap();

        assert_eq!(
            parsed,
            Control {
                operation_mode: OperationMode::Mailbox,
                direction: Direction::MasterWrite,
                ecat_event_enable: false,
                dls_user_event_enable: true,
                watchdog_enable: false,
                has_write_event: false,
                has_read_event: false,
                mailbox_full: false,
                buffer_state: BufferState::First,
                read_buffer_open: false,
                write_buffer_open: false,
            }
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
            0x26, 0x00, //
            // Enable
            0x01, 0x00,
        ];

        let parsed = SyncManagerChannel::unpack_from_slice(&raw).unwrap();

        assert_eq!(
            parsed,
            SyncManagerChannel {
                physical_start_address: 0x1000,
                length: 0x0080,
                control: Control {
                    operation_mode: OperationMode::Mailbox,
                    direction: Direction::MasterWrite,
                    ecat_event_enable: false,
                    dls_user_event_enable: true,
                    watchdog_enable: false,
                    has_write_event: false,
                    has_read_event: false,
                    mailbox_full: false,
                    buffer_state: BufferState::First,
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
