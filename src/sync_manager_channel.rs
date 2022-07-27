use packed_struct::prelude::*;

/// Sync manager channel.
///
/// Defined in ETG1000.4 6.7.2
#[derive(Copy, Clone, Debug, PartialEq, PackedStruct)]
#[packed_struct(byte_len = "8", bit_numbering = "msb0", endian = "lsb")]
pub struct SyncManagerChannel {
    #[packed_field(bits = "0..=15")]
    physical_start_address: u16,
    #[packed_field(bits = "16..=31")]
    length: u16,
    // ---
    // Next byte
    // ---
    #[packed_field(bits = "32..=33", ty = "enum")]
    buffer_type: EnumCatchAll<OperationMode>,
    #[packed_field(bits = "34..=35", ty = "enum")]
    direction: EnumCatchAll<Direction>,
    #[packed_field(bits = "36")]
    ecat_event_enable: bool,
    #[packed_field(bits = "37")]
    dls_user_event_enable: bool,
    #[packed_field(bits = "38")]
    watchdog_enable: bool,
    // #[packed_field(bits = "39")]
    // reserved1: bool
    // ---
    // Next byte
    // ---
    #[packed_field(bits = "40")]
    has_write_event: bool,
    #[packed_field(bits = "41")]
    has_read_event: bool,
    // #[packed_field(bits = "42")]
    // reserved1: bool
    #[packed_field(bits = "43")]
    mailbox_full: bool,
    #[packed_field(bits = "44..=45", ty = "enum")]
    buffer_state: EnumCatchAll<BufferState>,
    #[packed_field(bits = "46")]
    read_buffer_open: bool,
    #[packed_field(bits = "47")]
    write_buffer_open: bool,
    // ---
    // Next byte
    // ---
    #[packed_field(bits = "48")]
    channel_enabled: bool,
    #[packed_field(bits = "49")]
    repeat: bool,
    // #[packed_field(bits = "50..=53")]
    // reserved4: u8
    // TODO: Less insane names
    #[packed_field(bits = "54")]
    dc_event0w_busw: bool,
    #[packed_field(bits = "55")]
    dc_event0wlocw: bool,

    // Next byte
    #[packed_field(bits = "56")]
    channel_pdi_disabled: bool,
    #[packed_field(bits = "57")]
    repeat_ack: bool,
}

#[derive(Copy, Clone, Debug, PartialEq, PrimitiveEnum_u8)]
pub enum OperationMode {
    Buffered = 0x00,
    Mailbox = 0x02,
}

#[derive(Copy, Clone, Debug, PartialEq, PrimitiveEnum_u8)]
pub enum Direction {
    MasterRead = 0x00,
    MasterWrite = 0x01,
}

#[derive(Copy, Clone, Debug, PartialEq, PrimitiveEnum_u8)]
pub enum BufferState {
    First = 0x00,
    Second = 0x01,
    Third = 0x02,
    Locked = 0x03,
}
