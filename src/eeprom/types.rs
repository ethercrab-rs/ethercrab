//! Slave Information Interface (SII).

use core::fmt;

use crate::{
    base_data_types::PrimitiveDataType,
    mailbox::MailboxProtocols,
    sync_manager_channel::{self},
    PduRead,
};
use nom::{
    combinator::{map, map_opt, map_res},
    number::complete::{le_i16, le_u16, le_u8},
    IResult,
};
use num_enum::{FromPrimitive, TryFromPrimitive};
use packed_struct::prelude::*;

pub const TX_PDO_RANGE: core::ops::RangeInclusive<u16> = 0x1A00..=0x1bff;
pub const RX_PDO_RANGE: core::ops::RangeInclusive<u16> = 0x1600..=0x17ff;

/// Defined in ETG1000.4 6.4.2
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default, PackedStruct)]
#[packed_struct(size_bytes = "2", bit_numbering = "lsb0", endian = "lsb")]
pub struct SiiAccessConfig {
    // First byte, but second octet because little endian
    #[packed_field(bits = "8")]
    pub access_pdi: bool,
    // #[packed_field(bits = "9..=15")]
    // reserved7: u8,

    // Second byte, but first octet because little endian
    #[packed_field(bits = "0", ty = "enum")]
    pub owner: SiiOwner,
    #[packed_field(bits = "1")]
    pub lock: bool,
}

impl PduRead for SiiAccessConfig {
    const LEN: u16 = u16::LEN;

    type Error = PackingError;

    fn try_from_slice(slice: &[u8]) -> Result<Self, Self::Error> {
        Self::unpack_from_slice(slice)
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Default, PrimitiveEnum_u8)]
pub enum SiiOwner {
    /// EEPROM access rights are assigned to PDI during state change from Init to PreOp, Init to
    /// Boot and while in Boot
    #[default]
    Dl = 0x00,

    /// EEPROM access rights are assigned to PDI in all states except Init
    Pdi = 0x01,
}

/// Defined in ETG1000.4 6.4.3
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default, PackedStruct)]
#[packed_struct(size_bytes = "2", bit_numbering = "lsb0", endian = "lsb")]
pub struct SiiControl {
    // First byte, but second octet because little endian
    #[packed_field(bits = "8", ty = "enum")]
    pub access: SiiAccess,
    // #[packed_field(bits = "9..=12")]
    // reserved4: u8,
    #[packed_field(bits = "13")]
    pub emulate_sii: bool,
    #[packed_field(bits = "14", ty = "enum")]
    pub read_size: SiiReadSize,
    #[packed_field(bits = "15", ty = "enum")]
    pub address_type: SiiAddressSize,

    // Second byte, but first octet because little endian
    // TODO: Replace with bitflags struct?
    #[packed_field(bits = "0")]
    pub read: bool,
    #[packed_field(bits = "1")]
    pub write: bool,
    #[packed_field(bits = "2")]
    pub reload: bool,
    #[packed_field(bits = "3")]
    pub checksum_error: bool,
    #[packed_field(bits = "4")]
    pub device_info_error: bool,
    #[packed_field(bits = "5")]
    pub command_error: bool,
    #[packed_field(bits = "6")]
    pub write_error: bool,
    #[packed_field(bits = "7")]
    pub busy: bool,
}

impl SiiControl {
    pub fn has_error(&self) -> bool {
        self.checksum_error || self.device_info_error || self.command_error || self.write_error
    }

    pub fn error_reset(self) -> Self {
        Self {
            checksum_error: false,
            device_info_error: false,
            command_error: false,
            write_error: false,
            ..self
        }
    }

    fn read() -> Self {
        Self {
            read: true,
            ..Default::default()
        }
    }

    pub fn as_array(&self) -> [u8; 2] {
        self.pack().unwrap()
    }
}

impl PduRead for SiiControl {
    const LEN: u16 = u16::LEN;

    type Error = PackingError;

    fn try_from_slice(slice: &[u8]) -> Result<Self, Self::Error> {
        Self::unpack_from_slice(slice)
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Default, PrimitiveEnum_u8)]
pub enum SiiAccess {
    #[default]
    ReadOnly = 0x00,
    ReadWrite = 0x01,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Default, PrimitiveEnum_u8)]
pub enum SiiReadSize {
    /// Read 4 octets at a time.
    #[default]
    Octets4 = 0x00,

    /// Read 8 octets at a time.
    Octets8 = 0x01,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Default, PrimitiveEnum_u8)]
pub enum SiiAddressSize {
    #[default]
    U8 = 0x00,
    U16 = 0x01,
}

pub struct SiiRequest {
    control: SiiControl,
    address: u16,
}

impl fmt::Debug for SiiRequest {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SiiRequest")
            .field("control", &self.control)
            .field("address", &format_args!("{:#06x}", self.address))
            .finish()
    }
}

impl SiiRequest {
    pub fn read(address: u16) -> Self {
        Self {
            control: SiiControl::read(),
            address,
        }
    }

    pub fn as_array(&self) -> [u8; 6] {
        let mut buf = [0u8; 6];

        self.control.pack_to_slice(&mut buf[0..2]).unwrap();

        buf[2..4].copy_from_slice(&self.address.to_le_bytes());
        buf[4..6].copy_from_slice(&[0, 0]);

        buf
    }
}

/// SII register address.
///
/// Defined in ETG1000.6 Table 16 or ETG2010 Table 2
#[derive(Debug, num_enum::IntoPrimitive)]
#[repr(u16)]
pub enum SiiCoding {
    /// PDI Control
    // Unsigned16
    PdiControl = 0x0000,
    /// PDI Configuration
    // Unsigned16
    PdiConfiguration = 0x0001,
    /// SyncImpulseLen
    // Unsigned16
    SyncImpulseLen = 0x0002,
    /// PDI Configuration2
    ///
    /// Initialization value for PDI Configuration register R8 most significant word (0x152-0x153)
    // Unsigned16
    PdiConfiguration2 = 0x0003,
    /// Configured Station Alias
    // Unsigned16
    ConfiguredStationAlias = 0x0004,
    /// Checksum
    // Unsigned16
    Checksum = 0x0007,
    /// Vendor ID
    // Unsigned32
    VendorId = 0x0008,
    /// Product Code
    // Unsigned32
    ProductCode = 0x000A,
    /// Revision Number
    // Unsigned32
    RevisionNumber = 0x000C,
    /// Serial Number
    // Unsigned32
    SerialNumber = 0x000E,
    /// Reserved
    // BYTE
    Reserved = 0x0010,
    /// Bootstrap Receive Mailbox Offset
    // Unsigned16
    BootstrapReceiveMailboxOffset = 0x0014,
    /// Bootstrap Receive Mailbox Size
    // Unsigned16
    BootstrapReceiveMailboxSize = 0x0015,
    /// Bootstrap Send Mailbox Offset
    // Unsigned16
    BootstrapSendMailboxOffset = 0x0016,
    /// Bootstrap Send Mailbox Size
    // Unsigned16
    BootstrapSendMailboxSize = 0x0017,
    /// Standard Receive Mailbox Offset
    // Unsigned16
    StandardReceiveMailboxOffset = 0x0018,
    /// Standard Receive Mailbox Size
    // Unsigned16
    StandardReceiveMailboxSize = 0x0019,
    /// Standard Send Mailbox Offset
    // Unsigned16
    StandardSendMailboxOffset = 0x001A,
    /// Standard Send Mailbox Size
    // Unsigned16
    StandardSendMailboxSize = 0x001B,
    /// Mailbox Protocol - returns a [`MailboxProtocols`](crate::mailbox::MailboxProtocols).
    // Unsigned16
    MailboxProtocol = 0x001C,
    /// Size
    // Unsigned16
    Size = 0x003E,
    /// Version
    // Unsigned16
    Version = 0x003F,
}

/// Defined in ETG1000.6 Table 17
#[derive(Debug)]
pub struct SiiCategory {
    pub category: CategoryType,
    pub start: u16,

    /// Category length in words (`u16`)
    pub len_words: u16,
}

/// Defined in ETG1000.6 Table 19
#[derive(
    Debug, Copy, Clone, PartialEq, Eq, num_enum::TryFromPrimitive, num_enum::IntoPrimitive,
)]
#[repr(u16)]
pub enum CategoryType {
    Nop = 0,
    #[num_enum(alternatives = [2,3,4,5,6,7,8,9])]
    DeviceSpecific = 1,
    Strings = 10,
    DataTypes = 20,
    General = 30,
    Fmmu = 40,
    SyncManager = 41,
    FmmuExtended = 42,
    SyncUnit = 43,
    TxPdo = 50,
    RxPdo = 51,
    DistributedClock = 60,
    // TODO: Device specific 0x1000-0xfffe
    End = 0xffff,
}

/// ETG1000.6 Table 23
#[derive(
    Debug, Copy, Clone, PartialEq, Eq, num_enum::TryFromPrimitive, num_enum::IntoPrimitive,
)]
#[repr(u8)]
pub enum FmmuUsage {
    #[num_enum(alternatives = [0xff])]
    Unused = 0x00,
    Outputs = 0x01,
    Inputs = 0x02,
    SyncManagerStatus = 0x03,
}

/// ETG1020 Table 10 "FMMU_EX"
///
/// NOTE: Most fields defined are discarded from this struct as they are unused in Ethercrab.
// TODO: Rename to avoid conflict with crate::fmmu::Fmmu
#[derive(Debug, Copy, Clone)]
pub struct Fmmu {
    /// Sync manager index.
    pub sync_manager: u8,
}

impl Fmmu {
    pub fn parse(i: &[u8]) -> IResult<&[u8], Self> {
        let (i, _before) = le_u8(i)?;
        let (i, sync_manager) = le_u8(i)?;
        let (i, _after) = le_u8(i)?;

        Ok((i, Self { sync_manager }))
    }
}

/// SII "General" category.
///
/// Defined in ETG1000.6 Table 21
#[derive(Debug, PartialEq, Eq)]
#[allow(unused)]
pub struct SiiGeneral {
    group_string_idx: u8,
    image_string_idx: u8,
    order_string_idx: u8,
    pub name_string_idx: u8,
    // reserved: u8,
    coe_details: CoeDetails,
    foe_enabled: bool,
    eoe_enabled: bool,
    // Following 3 fields marked as reserved
    // soe_channels: u8,
    // ds402_channels: u8,
    // sysman_class: u8,
    flags: Flags,
    /// EBus Current Consumption in mA.
    ///
    /// A negative Values means feeding in current feed in sets the available current value to the
    /// given value
    ebus_current: i16,
    // reserved: u8,
    ports: [PortStatus; 4],
    /// defines the ESC memory address where the Identification ID is saved if Identification Method
    /// [`IDENT_PHY_M`] is set.
    physical_memory_addr: u16,
    // reserved2: [u8; 12]
}

impl SiiGeneral {
    pub fn parse(i: &[u8]) -> IResult<&[u8], Self> {
        let (i, group_string_idx) = le_u8(i)?;
        let (i, image_string_idx) = le_u8(i)?;
        let (i, order_string_idx) = le_u8(i)?;
        let (i, name_string_idx) = le_u8(i)?;
        let (i, _reserved) = le_u8(i)?;
        let (i, coe_details) = map_opt(le_u8, CoeDetails::from_bits)(i)?;
        let (i, foe_enabled) = map(le_u8, |num| num != 0)(i)?;
        let (i, eoe_enabled) = map(le_u8, |num| num != 0)(i)?;

        // Reserved, ignored
        let (i, _soe_channels) = le_u8(i)?;
        let (i, _ds402_channels) = le_u8(i)?;
        let (i, _sysman_class) = le_u8(i)?;

        let (i, flags) = map_opt(le_u8, Flags::from_bits)(i)?;
        let (i, ebus_current) = le_i16(i)?;

        let (i, ports) = map(le_u16, |raw| {
            let p1 = raw & 0x0f;
            let p2 = (raw >> 4) & 0x0f;
            let p3 = (raw >> 8) & 0x0f;
            let p4 = (raw >> 12) & 0x0f;

            [
                PortStatus::from_primitive(p1 as u8),
                PortStatus::from_primitive(p2 as u8),
                PortStatus::from_primitive(p3 as u8),
                PortStatus::from_primitive(p4 as u8),
            ]
        })(i)?;

        // let (i, physical_memory_addr) = le_u16(i)?;
        let physical_memory_addr = 0;

        Ok((
            i,
            Self {
                group_string_idx,
                image_string_idx,
                order_string_idx,
                name_string_idx,
                coe_details,
                foe_enabled,
                eoe_enabled,
                flags,
                ebus_current,
                ports,
                physical_memory_addr,
            },
        ))
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, num_enum::FromPrimitive)]
#[repr(u8)]
pub enum PortStatus {
    #[default]
    Unused = 0x00,
    Mii = 0x01,
    // TODO: Is this just a reserved value, not a port state?
    Reserved = 0x02,
    Ebus = 0x03,
    FastHotConnect = 0x04,
}

bitflags::bitflags! {
    pub struct Flags: u8 {
        const ENABLE_SAFE_OP = 0x01;
        const ENABLE_NOT_LRW = 0x02;
        const MAILBOX_DLL = 0x04;
        const IDENT_AL_STATUS = 0x08;
        const IDENT_PHY_M = 0x10;

    }
}

bitflags::bitflags! {
    pub struct CoeDetails: u8 {
        /// Bit 0: Enable SDO
        const ENABLE_SDO = 0x01;
        /// Bit 1: Enable SDO Info
        const ENABLE_SDO_INFO = 0x02;
        /// Bit 2: Enable PDO Assign
        const ENABLE_PDO_ASSIGN = 0x04;
        /// Bit 3: Enable PDO Configuration
        const ENABLE_PDO_CONFIG = 0x08;
        /// Bit 4: Enable Upload at startup
        const ENABLE_STARTUP_UPLOAD = 0x10;
        /// Bit 5: Enable SDO complete access
        const ENABLE_COMPLETE_ACCESS = 0x20;
    }
}

#[derive(Copy, Clone, PartialEq, Eq)]
pub struct SyncManager {
    pub(crate) start_addr: u16,
    pub(crate) length: u16,
    pub(crate) control: sync_manager_channel::Control,
    pub(crate) enable: SyncManagerEnable,
    pub(crate) usage_type: SyncManagerType,
}

impl fmt::Debug for SyncManager {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SyncManager")
            .field("start_addr", &format_args!("{:#06x}", self.start_addr))
            .field("length", &format_args!("{:#06x}", self.length))
            .field("control", &self.control)
            .field("enable", &self.enable)
            .field("usage_type", &self.usage_type)
            .finish()
    }
}

impl SyncManager {
    pub fn parse(i: &[u8]) -> IResult<&[u8], Self> {
        let (i, start_addr) = le_u16(i)?;
        let (i, length) = le_u16(i)?;
        let (i, control) =
            map_res(le_u8, |byte| sync_manager_channel::Control::unpack(&[byte]))(i)?;

        // Ignored
        let (i, _status) = le_u8(i)?;

        let (i, enable) = map_opt(le_u8, SyncManagerEnable::from_bits)(i)?;
        let (i, usage_type) = map(le_u8, SyncManagerType::from_primitive)(i)?;

        Ok((
            i,
            Self {
                start_addr,
                length,
                control,
                enable,
                usage_type,
            },
        ))
    }
}

bitflags::bitflags! {
    pub struct SyncManagerEnable: u8 {
        /// Bit 0: enable.
        const ENABLE = 0x01;
        /// Bit 1: fixed content (info for config tool –SyncMan has fixed content).
        const IS_FIXED = 0x02;
        /// Bit 2: virtual SyncManager (virtual SyncMan – no hardware resource used).
        const IS_VIRTUAL = 0x04;
        /// Bit 3: opOnly (SyncMan should be enabled only in OP state).
        const OP_ONLY = 0x08;
    }
}

#[derive(Debug, Copy, Clone, Default, PartialEq, Eq, num_enum::FromPrimitive)]
#[repr(u8)]
pub enum SyncManagerType {
    /// Not used or unknown.
    #[default]
    Unknown = 0x00,
    /// Used for mailbox out.
    MailboxOut = 0x01,
    /// Used for mailbox in.
    MailboxIn = 0x02,
    /// Used for process data outputs from master.
    ProcessDataWrite = 0x03,
    /// Used for process data inputs to master.
    ProcessDataRead = 0x04,
}

/// Defined in ETG2010 Table 14 – Structure Category TXPDO and RXPDO for each PDO
#[derive(Clone)]
// TODO: Remove
#[allow(unused)]
pub struct Pdo {
    pub(crate) index: u16,
    pub(crate) num_entries: u8,
    // TODO: Un-pub
    pub sync_manager: u8,
    dc_sync: u8,
    /// Index into EEPROM Strings section for PDO name.
    name_string_idx: u8,
    flags: PdoFlags,
    pub(crate) entries: heapless::Vec<PdoEntry, 16>,
}

impl fmt::Debug for Pdo {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Pdo")
            .field("index", &format_args!("{:#06x}", self.index))
            .field("num_entries", &self.num_entries)
            .field("sync_manager", &self.sync_manager)
            .field("dc_sync", &self.dc_sync)
            .field("name_string_idx", &self.name_string_idx)
            .field("flags", &self.flags)
            .field("entries", &self.entries)
            .finish()
    }
}

impl Pdo {
    pub fn parse(i: &[u8]) -> IResult<&[u8], Self> {
        let (i, index) = le_u16(i)?;
        let (i, num_entries) = le_u8(i)?;
        let (i, sync_manager) = le_u8(i)?;
        let (i, dc_sync) = le_u8(i)?;
        let (i, name_string_idx) = le_u8(i)?;
        let (i, flags) = map_opt(le_u16, PdoFlags::from_bits)(i)?;

        Ok((
            i,
            Self {
                index,
                num_entries,
                sync_manager,
                dc_sync,
                name_string_idx,
                flags,
                entries: heapless::Vec::new(),
            },
        ))
    }

    /// Compute the total bit length of this PDO by iterating over and summing the bit length of
    /// each entry contained within.
    pub fn bit_len(&self) -> u16 {
        self.entries
            .iter()
            .map(|entry| u16::from(entry.data_length_bits))
            .sum()
    }
}

#[derive(Clone)]
// TODO: Remove
#[allow(unused)]
pub struct PdoEntry {
    index: u16,
    sub_index: u8,
    name_string_idx: u8,
    // See page 103 of ETG2000
    data_type: PrimitiveDataType,
    // TODO: Un-pub
    pub(crate) data_length_bits: u8,
    flags: u16,
}

impl fmt::Debug for PdoEntry {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("PdoEntry")
            .field("index", &format_args!("{:#06x}", self.index))
            .field("sub_index", &self.sub_index)
            .field("name_string_idx", &self.name_string_idx)
            .field("data_type", &self.data_type)
            .field("data_length_bits", &self.data_length_bits)
            .field("flags", &self.flags)
            .finish()
    }
}

impl PdoEntry {
    // TODO: `all_consuming`, and for all other `parse()` methods
    pub fn parse(i: &[u8]) -> IResult<&[u8], Self> {
        let (i, index) = le_u16(i)?;
        let (i, sub_index) = le_u8(i)?;
        let (i, name_string_idx) = le_u8(i)?;
        let (i, data_type) = map_res(le_u8, PrimitiveDataType::try_from_primitive)(i)?;
        let (i, data_length_bits) = le_u8(i)?;
        let (i, flags) = le_u16(i)?;

        Ok((
            i,
            Self {
                index,
                sub_index,
                name_string_idx,
                data_type,
                data_length_bits,
                flags,
            },
        ))
    }
}

bitflags::bitflags! {
    pub struct PdoFlags: u16 {
        /// PdoMandatory [Esi:RTxPdo@Mandatory]
        const PDO_MANDATORY = 0x0001;
        /// PdoDefault [Esi:RTxPdo@Sm]
        const PDO_DEFAULT = 0x0002;
        /// Reserved (PdoOversample)
        const PDO_OVERSAMPLE = 0x0004;
        /// PdoFixedContent [Esi:RTxPdo@Fixed]
        const PDO_FIXED_CONTENT = 0x0010;
        /// PdoVirtualContent [Esi:RTxPdo@Virtual]
        const PDO_VIRTUAL_CONTENT = 0x0020;
        /// Reserved (PdoDownloadAnyway)
        const PDO_DOWNLOAD_ANYWAY = 0x0040;
        /// Reserved (PdoFromModule)
        const PDO_FROM_MODULE = 0x0080;
        /// PdoModuleAlign [Esi:Slots:ModulePdoGroup@Alignment]
        const PDO_MODULE_ALIGN = 0x0100;
        /// PdoDependOnSlot [Esi:RTxPdo:Index@DependOnSlot]
        const PDO_DEPEND_ON_SLOT = 0x0200;
        /// PdoDependOnSlotGroup [Esi:RTxPdo:Index@DependOnSlotGroup]
        const PDO_DEPEND_ON_SLOT_GROUP = 0x0400;
        /// PdoOverwrittenByModule [Esi:RTxPdo@OverwrittenByModule]
        const PDO_OVERWRITTEN_BY_MODULE = 0x0800;
        /// Reserved (PdoConfigurable)
        const PDO_CONFIGURABLE = 0x1000;
        /// Reserved (PdoAutoPdoName)
        const PDO_AUTO_PDO_NAME = 0x2000;
        /// Reserved (PdoDisAutoExclude)
        const PDO_DIS_AUTO_EXCLUDE = 0x4000;
        /// Reserved (PdoWritable)
        const PDO_WRITABLE = 0x8000;
    }
}

#[derive(Clone)]
pub struct MailboxConfig {
    pub receive_offset: u16,
    pub receive_size: u16,
    pub send_offset: u16,
    pub send_size: u16,
    pub protocol: MailboxProtocols,
}

impl fmt::Debug for MailboxConfig {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("MailboxConfig")
            .field(
                "receive_offset",
                &format_args!("{:#06x}", self.receive_offset),
            )
            .field("receive_size", &format_args!("{:#06x}", self.receive_size))
            .field("send_offset", &format_args!("{:#06x}", self.send_offset))
            .field("send_size", &format_args!("{:#06x}", self.send_size))
            .field("protocol", &self.protocol)
            .finish()
    }
}

impl MailboxConfig {
    // TODO: `all_consuming`, and for all other `parse()` methods
    pub fn parse(i: &[u8]) -> IResult<&[u8], Self> {
        let (i, receive_offset) = le_u16(i)?;
        let (i, receive_size) = le_u16(i)?;
        let (i, send_offset) = le_u16(i)?;
        let (i, send_size) = le_u16(i)?;
        let (i, protocol) = map_opt(le_u16, MailboxProtocols::from_bits)(i)?;

        Ok((
            i,
            Self {
                receive_offset,
                receive_size,
                send_offset,
                send_size,
                protocol,
            },
        ))
    }
}
