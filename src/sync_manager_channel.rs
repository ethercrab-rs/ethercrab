use core::fmt;
use ethercrab_wire::{EtherCatWire, WireError};

/// ETG1000.6 Table 67 – CoE Communication Area, "Sync Manager Communication Type".
pub const SM_TYPE_ADDRESS: u16 = 0x1c00;

/// // ETG1000.6 Table 67 – CoE Communication Area, the address of the first sync manager.
pub const SM_BASE_ADDRESS: u16 = 0x1c10;

/// Sync manager channel.
///
/// Defined in ETG1000.4 6.7.2
#[derive(Default, Copy, Clone, PartialEq, Eq, ethercrab_wire::EtherCatWire)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[wire(bits = 64)]
pub struct SyncManagerChannel {
    #[wire(bytes = 2)]
    pub physical_start_address: u16,
    #[wire(bytes = 2)]
    pub length_bytes: u16,
    #[wire(bytes = 1)]
    pub control: Control,
    #[wire(bytes = 1)]
    pub status: Status,
    #[wire(bytes = 2)]
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

#[derive(Default, Copy, Clone, Debug, PartialEq, Eq, ethercrab_wire::EtherCatWire)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[wire(bits = 8)]
pub struct Control {
    #[wire(bits = 2)]
    pub operation_mode: OperationMode,
    #[wire(bits = 2)]
    pub direction: Direction,
    #[wire(bits = 1)]
    pub ecat_event_enable: bool,
    #[wire(bits = 1)]
    pub dls_user_event_enable: bool,
    #[wire(bits = 1, post_skip = 1)]
    pub watchdog_enable: bool,
    // reserved1: bool
}

#[derive(Default, Copy, Clone, Debug, PartialEq, Eq, ethercrab_wire::EtherCatWire)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[wire(bits = 8)]
pub struct Status {
    #[wire(bits = 1)]
    pub has_write_event: bool,
    #[wire(bits = 1, post_skip = 1)]
    pub has_read_event: bool,
    // reserved1: bool
    #[wire(bits = 1)]
    pub mailbox_full: bool,
    #[wire(bits = 2)]
    pub buffer_state: BufferState,
    #[wire(bits = 1)]
    pub read_buffer_open: bool,
    #[wire(bits = 1)]
    pub write_buffer_open: bool,
}

/// Described in ETG1000.4 6.7.2 Sync Manager Attributes
#[derive(Default, Copy, Clone, Debug, PartialEq, Eq, ethercrab_wire::EtherCatWire)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[wire(bits = 16)]
pub struct Enable {
    #[wire(bits = 1)]
    pub enable: bool,
    #[wire(bits = 1, post_skip = 4)]
    pub repeat: bool,
    // reserved4: u8
    /// DC Event 0 with EtherCAT write.
    ///
    /// Set to `true` to enable DC 0 events on EtherCAT writes.
    #[wire(bits = 1)]
    pub enable_dc_event_bus_write: bool,

    /// DC Event 0 with local write.
    ///
    /// Set to `true` to enable DC 0 events from local writes.
    #[wire(bits = 1)]
    pub enable_dc_event_local_write: bool,
    // ---
    // Second byte (little endian, so first index)
    // ---
    #[wire(bits = 1)]
    pub channel_pdi_disabled: bool,
    #[wire(bits = 1, post_skip = 6)]
    pub repeat_ack: bool,
    // reserved6: u8,
}

#[derive(Default, Copy, Clone, Debug, PartialEq, Eq, ethercrab_wire::EtherCatWire)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[wire(bits = 2)]
#[repr(u8)]
pub enum OperationMode {
    #[default]
    Normal = 0x00,
    Mailbox = 0x02,
}

#[derive(Default, Copy, Clone, Debug, PartialEq, Eq, ethercrab_wire::EtherCatWire)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[wire(bits = 2)]
#[repr(u8)]
pub enum Direction {
    #[default]
    MasterRead = 0x00,
    MasterWrite = 0x01,
}

/// Buffer state.
///
/// Somewhat described in ETG1000.4 Figure 32 – SyncM mailbox interaction.
///
/// In cyclic mode the buffers need to be tripled. It's unclear why from the spec but that's what it
/// says.
#[derive(Default, Copy, Clone, Debug, PartialEq, Eq, ethercrab_wire::EtherCatWire)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[wire(bits = 2)]
#[repr(u8)]
pub enum BufferState {
    /// First buffer.
    #[default]
    First = 0x00,
    /// Second buffer.
    Second = 0x01,
    /// Third buffer.
    Third = 0x02,
    /// Next buffer.
    Next = 0x03,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn issue_49_decode_timeout_response() {
        let raw = [0x00u8, 0x1c, 0x00, 0x01, 0x22, 0x00, 0x01, 0x00];

        let parsed = SyncManagerChannel::unpack_from_slice(&raw).unwrap();

        assert_eq!(
            parsed,
            SyncManagerChannel {
                physical_start_address: 0x1c00,
                length_bytes: 0x0100,
                control: Control {
                    operation_mode: OperationMode::Mailbox,
                    direction: Direction::MasterRead,
                    ecat_event_enable: false,
                    dls_user_event_enable: true,
                    watchdog_enable: false,
                },
                status: Status {
                    has_write_event: false,
                    has_read_event: false,
                    mailbox_full: false,
                    buffer_state: BufferState::First,
                    read_buffer_open: false,
                    write_buffer_open: false
                },
                enable: Enable {
                    enable: true,
                    repeat: false,
                    enable_dc_event_bus_write: false,
                    enable_dc_event_local_write: false,
                    channel_pdi_disabled: false,
                    repeat_ack: false
                }
            }
        )
    }

    #[test]
    fn default_is_zero() {
        // MSRV: `generic_const_exprs`
        // assert_eq!(SyncManagerChannel::default().pack(), [0u8; 8]);
    }

    #[test]
    fn size() {
        // Packed size
        assert_eq!(SyncManagerChannel::default().packed_len(), 8);
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
                enable_dc_event_bus_write: false,
                enable_dc_event_local_write: false,
                channel_pdi_disabled: false,
                repeat_ack: false,
            }
        )
    }

    #[test]
    fn decode_mailbox_event() {
        let raw = [0x09];

        let parsed = Status::unpack_from_slice(&raw).unwrap();

        assert!(parsed.mailbox_full)
    }

    #[test]
    fn encode_enable() {
        let mut buf = [0u8; 2];

        let raw = Enable {
            enable: true,
            repeat: false,
            enable_dc_event_bus_write: false,
            enable_dc_event_local_write: false,
            channel_pdi_disabled: false,
            repeat_ack: false,
        }
        .pack_to_slice(&mut buf)
        .unwrap();

        assert_eq!(raw, &[0x01, 0x00])
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
                    buffer_state: BufferState::First,
                    read_buffer_open: false,
                    write_buffer_open: false,
                },
                enable: Enable {
                    enable: true,
                    repeat: false,
                    enable_dc_event_bus_write: false,
                    enable_dc_event_local_write: false,
                    channel_pdi_disabled: false,
                    repeat_ack: false,
                }
            }
        )
    }
}
