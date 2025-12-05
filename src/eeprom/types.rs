//! SubDevice Information Interface (SII).

use crate::{
    coe::SdoExpedited,
    sync_manager_channel::{self, Direction, OperationMode},
};
use ethercrab_wire::{EtherCrabWireRead, EtherCrabWireSized};

#[derive(Debug, Copy, Clone, PartialEq, Eq, Default, ethercrab_wire::EtherCrabWireReadWrite)]
#[repr(u8)]
pub enum SiiOwner {
    /// EEPROM access rights are assigned to PDI during state change from Init to PreOp, Init to
    /// Boot and while in Boot
    #[default]
    Master = 0x00,

    /// EEPROM access rights are assigned to PDI in all states except Init
    Pdi = 0x01,
}

/// Defined in ETG1000.4 6.4.3
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default, ethercrab_wire::EtherCrabWireReadWrite)]
#[wire(bytes = 2)]
pub struct SiiControl {
    // First byte
    #[wire(bits = 1)]
    pub access: SiiAccess,
    // reserved4: u8,
    #[wire(pre_skip = 4, bits = 1)]
    pub emulate_sii: bool,
    #[wire(bits = 1)]
    pub read_size: SiiReadSize,
    #[wire(bits = 1)]
    pub address_type: SiiAddressSize,

    // Second byte
    #[wire(bits = 1)]
    pub read: bool,
    #[wire(bits = 1)]
    pub write: bool,
    #[wire(bits = 1)]
    pub reload: bool,
    #[wire(bits = 1)]
    pub checksum_error: bool,
    #[wire(bits = 1)]
    pub device_info_error: bool,
    // NOTE: This comes back as `1` when setting the station alias, however the alias is set
    // correctly on EK1100, and the same behaviour happens with SOEM's `eepromtool` as well, so I
    // don't know what this field is for/does.
    #[wire(bits = 1)]
    pub command_error: bool,
    #[wire(bits = 1)]
    pub write_error: bool,
    #[wire(bits = 1)]
    pub busy: bool,
}

impl SiiControl {
    pub fn has_error(&self) -> bool {
        self.checksum_error || self.device_info_error || self.write_error
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

    fn write() -> Self {
        Self {
            access: SiiAccess::ReadWrite,
            write: true,
            ..Default::default()
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Default, ethercrab_wire::EtherCrabWireReadWrite)]
#[repr(u8)]
pub enum SiiAccess {
    #[default]
    ReadOnly = 0x00,
    ReadWrite = 0x01,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Default, ethercrab_wire::EtherCrabWireReadWrite)]
#[repr(u8)]
pub enum SiiReadSize {
    /// Read 4 octets at a time.
    #[default]
    Octets4 = 0x00,

    /// Read 8 octets at a time.
    Octets8 = 0x01,
}

impl SiiReadSize {
    pub fn chunk_len(&self) -> u16 {
        match self {
            SiiReadSize::Octets4 => 4,
            SiiReadSize::Octets8 => 8,
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Default, ethercrab_wire::EtherCrabWireReadWrite)]
#[repr(u8)]
pub enum SiiAddressSize {
    #[default]
    U8 = 0x00,
    U16 = 0x01,
}

#[derive(PartialEq, ethercrab_wire::EtherCrabWireReadWrite)]
#[wire(bytes = 6)]
pub struct SiiRequest {
    #[wire(bytes = 2)]
    control: SiiControl,
    // Post skip is required to send the correct amount of bytes on the wire. This is weird because
    // addressing is all a single WORD, but the SII read request expects a low AND high WORD, hence
    // the extra 16 bits of padding here for the unusedhigh WORD.
    #[wire(bytes = 2, post_skip = 16)]
    address: u16,
}

impl core::fmt::Debug for SiiRequest {
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

    pub fn write(address: u16) -> Self {
        Self {
            control: SiiControl::write(),
            address,
        }
    }
}

/// SII register address.
///
/// Defined in ETG1000.6 Table 16 or ETG2010 Table 2
#[derive(Debug, Copy, Clone, ethercrab_wire::EtherCrabWireRead)]
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

/// Defined in ETG1000.6 Table 19.
///
/// Additional information also in ETG1000.6 Table 17.
#[derive(Default, Debug, Copy, Clone, PartialEq, Eq, ethercrab_wire::EtherCrabWireRead)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[repr(u16)]
pub enum CategoryType {
    #[default]
    Nop = 0,
    #[wire(alternatives = [2,3,4,5,6,7,8,9])]
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
    // Device specific: 0x1000-0xfffe
    End = 0xffff,
}

/// The type of PDO to search for.
#[derive(Debug, Copy, Clone)]
pub enum PdoType {
    /// SubDevice send, MainDevice receive.
    Tx = 50,

    /// SubDevice receive, MainDevice send.
    Rx = 51,
}

impl From<PdoType> for CategoryType {
    fn from(value: PdoType) -> Self {
        match value {
            PdoType::Tx => Self::TxPdo,
            PdoType::Rx => Self::RxPdo,
        }
    }
}

/// ETG1000.6 Table 23
#[derive(Debug, Copy, Clone, PartialEq, Eq, ethercrab_wire::EtherCrabWireRead)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[repr(u8)]
pub enum FmmuUsage {
    #[wire(alternatives = [0xff])]
    Unused = 0x00,
    Outputs = 0x01,
    Inputs = 0x02,
    SyncManagerStatus = 0x03,
}

/// ETG1020 Table 10 "FMMU_EX"
///
/// NOTE: Most fields defined are discarded from this struct as they are unused in Ethercrab.
#[derive(Debug, Copy, Clone, PartialEq, ethercrab_wire::EtherCrabWireRead)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[wire(bytes = 3)]
pub struct FmmuEx {
    /// Sync manager index.
    #[wire(pre_skip_bytes = 1, bytes = 1, post_skip_bytes = 1)]
    pub sync_manager: u8,
}

#[derive(Default, Copy, Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct PortStatuses(pub [PortStatus; 4]);

impl EtherCrabWireSized for PortStatuses {
    const PACKED_LEN: usize = 2;

    type Buffer = [u8; Self::PACKED_LEN];

    fn buffer() -> Self::Buffer {
        [0u8; Self::PACKED_LEN]
    }
}

impl EtherCrabWireRead for PortStatuses {
    fn unpack_from_slice(buf: &[u8]) -> Result<Self, ethercrab_wire::WireError> {
        // Remember: little endian
        let Some(&[lo, hi]) = buf.get(0..Self::PACKED_LEN) else {
            return Err(ethercrab_wire::WireError::ReadBufferTooShort);
        };

        let p1 = lo & 0x0f;
        let p2 = (lo >> 4) & 0x0f;
        let p3 = hi & 0x0f;
        let p4 = (hi >> 4) & 0x0f;

        Ok(Self([
            PortStatus::from(p1),
            PortStatus::from(p2),
            PortStatus::from(p3),
            PortStatus::from(p4),
        ]))
    }
}

/// SII "General" category.
///
/// Defined in ETG1000.6 Table 21
#[derive(Debug, Default, PartialEq, Eq, ethercrab_wire::EtherCrabWireRead)]
#[wire(bytes = 18)]
pub struct SiiGeneral {
    #[wire(bytes = 1)]
    pub(crate) group_string_idx: u8,
    #[wire(bytes = 1)]
    pub(crate) image_string_idx: u8,
    #[wire(bytes = 1)]
    pub(crate) order_string_idx: u8,
    #[wire(bytes = 1, post_skip_bytes = 1)]
    pub name_string_idx: u8,
    // reserved: u8,
    #[wire(bytes = 1)]
    pub coe_details: CoeDetails,
    #[wire(bytes = 1)]
    pub(crate) foe_enabled: bool,
    #[wire(bytes = 1, post_skip_bytes = 3)]
    pub(crate) eoe_enabled: bool,
    // Following 3 fields marked as reserved
    // soe_channels: u8,
    // ds402_channels: u8,
    // sysman_class: u8,
    #[wire(bytes = 1)]
    pub(crate) flags: Flags,
    /// EBus Current Consumption in mA.
    ///
    /// A negative Values means feeding in current feed in sets the available current value to the
    /// given value
    #[wire(bytes = 2)]
    pub(crate) ebus_current: i16,
    // reserved: u8,
    #[wire(bytes = 2)]
    pub(crate) ports: PortStatuses,
    /// defines the ESC memory address where the Identification ID is saved if Identification Method
    /// [`IDENT_PHY_M`] is set.
    #[wire(bytes = 2)]
    pub(crate) physical_memory_addr: u16,
    // reserved2: [u8; 12]
}

#[derive(Default, Debug, Copy, Clone, PartialEq, Eq, ethercrab_wire::EtherCrabWireRead)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[repr(u8)]
pub enum PortStatus {
    #[default]
    Unused = 0x00,
    Mii = 0x01,
    Reserved = 0x02,
    Ebus = 0x03,
    FastHotConnect = 0x04,
}

bitflags::bitflags! {
    #[derive(Debug, Default, PartialEq, Eq)]
    pub struct Flags: u8 {
        const ENABLE_SAFE_OP = 0x01;
        const ENABLE_NOT_LRW = 0x02;
        const MAILBOX_DLL = 0x04;
        const IDENT_AL_STATUS = 0x08;
        const IDENT_PHY_M = 0x10;

    }
}

impl EtherCrabWireSized for Flags {
    const PACKED_LEN: usize = 1;

    type Buffer = [u8; Self::PACKED_LEN];

    fn buffer() -> Self::Buffer {
        [0u8; Self::PACKED_LEN]
    }
}

impl EtherCrabWireRead for Flags {
    fn unpack_from_slice(buf: &[u8]) -> Result<Self, ethercrab_wire::WireError> {
        u8::unpack_from_slice(buf)
            .and_then(|value| Self::from_bits(value).ok_or(ethercrab_wire::WireError::InvalidValue))
    }
}

bitflags::bitflags! {
    #[derive(Debug, Default, PartialEq, Eq)]
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

impl EtherCrabWireSized for CoeDetails {
    const PACKED_LEN: usize = 1;

    type Buffer = [u8; Self::PACKED_LEN];

    fn buffer() -> Self::Buffer {
        [0u8; Self::PACKED_LEN]
    }
}

impl EtherCrabWireRead for CoeDetails {
    fn unpack_from_slice(buf: &[u8]) -> Result<Self, ethercrab_wire::WireError> {
        u8::unpack_from_slice(buf)
            .and_then(|value| Self::from_bits(value).ok_or(ethercrab_wire::WireError::InvalidValue))
    }
}

#[derive(Copy, Clone, PartialEq, Eq, ethercrab_wire::EtherCrabWireRead)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[wire(bytes = 8)]
pub struct SyncManager {
    #[wire(bytes = 2)]
    pub(crate) start_addr: u16,
    #[wire(bytes = 2)]
    pub(crate) length: u16,
    #[wire(bytes = 1, post_skip_bytes = 1)]
    pub(crate) control: sync_manager_channel::Control,
    #[wire(bytes = 1)]
    pub(crate) enable: SyncManagerEnable,
    /// Usage type.
    ///
    /// Use the method of the same name instead of directly accessing this field. It is only exposed
    /// for test purposes.
    #[wire(bytes = 1)]
    pub(crate) usage_type: SyncManagerType,
}

impl SyncManager {
    pub(crate) fn usage_type(&self) -> SyncManagerType {
        if self.usage_type != SyncManagerType::Unknown {
            self.usage_type
        } else {
            // Try to recover type by matching on other fields in the SM
            match (self.control.operation_mode, self.control.direction) {
                (OperationMode::Normal, Direction::MasterRead) => SyncManagerType::ProcessDataRead,
                (OperationMode::Normal, Direction::MasterWrite) => {
                    SyncManagerType::ProcessDataWrite
                }
                (OperationMode::Mailbox, Direction::MasterRead) => SyncManagerType::MailboxRead,
                (OperationMode::Mailbox, Direction::MasterWrite) => SyncManagerType::MailboxWrite,
            }
        }
    }
}

impl core::fmt::Debug for SyncManager {
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

bitflags::bitflags! {
    #[derive(Debug, Copy, Clone, PartialEq, Eq)]
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

impl EtherCrabWireSized for SyncManagerEnable {
    const PACKED_LEN: usize = 1;

    type Buffer = [u8; Self::PACKED_LEN];

    fn buffer() -> Self::Buffer {
        [0u8; Self::PACKED_LEN]
    }
}

impl EtherCrabWireRead for SyncManagerEnable {
    fn unpack_from_slice(buf: &[u8]) -> Result<Self, ethercrab_wire::WireError> {
        u8::unpack_from_slice(buf)
            .and_then(|value| Self::from_bits(value).ok_or(ethercrab_wire::WireError::InvalidValue))
    }
}

// Can't derive, so manual impl
#[cfg(feature = "defmt")]
impl defmt::Format for SyncManagerEnable {
    fn format(&self, f: defmt::Formatter) {
        defmt::write!(f, "{=u8:b}", self.bits())
    }
}

#[derive(Default, Debug, Copy, Clone, PartialEq, Eq, ethercrab_wire::EtherCrabWireRead)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[repr(u8)]
pub enum SyncManagerType {
    /// Not used or unknown.
    #[default]
    Unknown = 0x00,
    /// Used for writing into the SubDevice.
    MailboxWrite = 0x01,
    /// Used for reading from the SubDevice.
    MailboxRead = 0x02,
    /// Used for process data outputs from MainDevice.
    ProcessDataWrite = 0x03,
    /// Used for process data inputs to MainDevice.
    ProcessDataRead = 0x04,
}

impl SdoExpedited for SyncManagerType {}

/// Defined in ETG2010 Table 14 – Structure Category TXPDO and RXPDO for each PDO
#[derive(Debug, Copy, Clone, PartialEq, ethercrab_wire::EtherCrabWireRead)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[wire(bytes = 8)]
pub struct Pdo {
    // #[wire(bytes = 2)]
    // pub(crate) index: u16,
    #[wire(bytes = 1, pre_skip_bytes = 2)]
    pub num_entries: u8,
    #[wire(bytes = 1, post_skip_bytes = 4)]
    pub sync_manager: u8,
    // #[wire(bytes = 1)]
    // pub(crate) dc_sync: u8,
    // /// Index into EEPROM Strings section for PDO name.
    // #[wire(bytes = 1)]
    // pub(crate) name_string_idx: u8,
    // #[wire(bytes = 2)]
    // pub(crate) flags: PdoFlags,

    // NOTE: Field is only used to sum up `bit_len`, so we don't need to read or store it.
    // Definition is left here in case we need it later.
    // // NOTE: This field is skipped during parsing from the wire and is populated later.
    // #[wire(skip)]
    // pub(crate) entries: heapless::Vec<PdoEntry, 16>,

    // NOTE: This field is skipped during parsing from the wire and is populated from all the
    // `PdoEntry`s later.
    #[wire(skip)]
    pub bit_len: u16,
}

// impl core::fmt::Debug for Pdo {
//     fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
//         f.debug_struct("Pdo")
//             // .field("index", &format_args!("{:#06x}", self.index))
//             .field("num_entries", &self.num_entries)
//             // .field("sync_manager", &self.sync_manager)
//             // .field("dc_sync", &self.dc_sync)
//             // .field("name_string_idx", &self.name_string_idx)
//             // .field("flags", &self.flags)
//             // .field("entries", &self.entries)
//             .field("bit_len", &self.bit_len)
//             .finish()
//     }
// }

// impl Pdo {
//     /// Compute the total bit length of this PDO by iterating over and summing the bit length of
//     /// each entry contained within.
//     pub fn bit_len(&self) -> u16 {
//         self.entries
//             .iter()
//             .map(|entry| u16::from(entry.data_length_bits))
//             .sum()
//     }
// }

#[derive(Debug, Clone, PartialEq, ethercrab_wire::EtherCrabWireRead)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[wire(bytes = 8)]
pub struct PdoEntry {
    // #[wire(bytes = 2)]
    // pub(crate) index: u16,
    // #[wire(bytes = 1)]
    // pub(crate) sub_index: u8,
    // #[wire(bytes = 1)]
    // pub(crate) name_string_idx: u8,
    // // See page 103 of ETG2000
    // #[wire(bytes = 1)]
    // pub(crate) data_type: PrimitiveDataType,
    #[wire(bytes = 1, pre_skip_bytes = 5, post_skip_bytes = 2)]
    pub data_length_bits: u8,
    // #[wire(bytes = 2)]
    // pub(crate) flags: u16,
}

// impl core::fmt::Debug for PdoEntry {
//     fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
//         f.debug_struct("PdoEntry")
//             .field("index", &format_args!("{:#06x}", self.index))
//             .field("sub_index", &self.sub_index)
//             .field("name_string_idx", &self.name_string_idx)
//             .field("data_type", &self.data_type)
//             .field("data_length_bits", &self.data_length_bits)
//             .field("flags", &self.flags)
//             .finish()
//     }
// }

bitflags::bitflags! {
    /// Defined in ETG2010 Table 14 offset 0x0006.
    #[derive(Copy, Clone, Debug, PartialEq)]
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

impl EtherCrabWireSized for PdoFlags {
    const PACKED_LEN: usize = 2;

    type Buffer = [u8; Self::PACKED_LEN];

    fn buffer() -> Self::Buffer {
        [0u8; Self::PACKED_LEN]
    }
}

impl EtherCrabWireRead for PdoFlags {
    fn unpack_from_slice(buf: &[u8]) -> Result<Self, ethercrab_wire::WireError> {
        u16::unpack_from_slice(buf)
            .and_then(|value| Self::from_bits(value).ok_or(ethercrab_wire::WireError::InvalidValue))
    }
}

// Can't derive, so manual impl
#[cfg(feature = "defmt")]
impl defmt::Format for PdoFlags {
    fn format(&self, f: defmt::Formatter) {
        defmt::write!(f, "{=u16:b}", self.bits())
    }
}

bitflags::bitflags! {
    /// Supported mailbox category.
    ///
    /// Defined in ETG1000.6 Table 18 or ETG2010 Table 4.
    // NOTE: Is actually a u16, but only the lower byte has any data in it
    #[derive(Copy, Clone, Default, Debug, PartialEq)]
    pub struct MailboxProtocols: u8 {
        /// ADS over EtherCAT (routing and parallel services).
        const AOE = 0x01;
        /// Ethernet over EtherCAT (tunnelling of Data Link services).
        const EOE = 0x02;
        /// CAN application protocol over EtherCAT (access to SDO).
        const COE = 0x04;
        /// File Access over EtherCAT.
        const FOE = 0x08;
        /// Servo Drive Profile over EtherCAT.
        const SOE = 0x10;
        /// Vendor specific protocol over EtherCAT.
        const VOE = 0x20;
    }
}

impl EtherCrabWireSized for MailboxProtocols {
    const PACKED_LEN: usize = 2;

    type Buffer = [u8; Self::PACKED_LEN];

    fn buffer() -> Self::Buffer {
        [0u8; Self::PACKED_LEN]
    }
}

impl EtherCrabWireRead for MailboxProtocols {
    fn unpack_from_slice(buf: &[u8]) -> Result<Self, ethercrab_wire::WireError> {
        // NOTE: Is actually a u16, but only the lower byte has any data in it
        buf.first()
            .ok_or(ethercrab_wire::WireError::ReadBufferTooShort)
            .and_then(|res| Self::from_bits(*res).ok_or(ethercrab_wire::WireError::InvalidValue))
    }
}

// Can't derive, so manual impl
#[cfg(feature = "defmt")]
impl defmt::Format for MailboxProtocols {
    fn format(&self, f: defmt::Formatter) {
        defmt::write!(f, "{=u8:b}", self.bits())
    }
}

#[derive(Copy, Clone, Default, PartialEq, ethercrab_wire::EtherCrabWireRead)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[wire(bytes = 10)]
pub struct DefaultMailbox {
    /// MainDevice to SubDevice receive mailbox address offset.
    #[wire(bytes = 2)]
    pub subdevice_receive_offset: u16,
    /// MainDevice to SubDevice receive mailbox size.
    #[wire(bytes = 2)]
    pub subdevice_receive_size: u16,
    /// SubDevice to MainDevice send mailbox address offset.
    #[wire(bytes = 2)]
    pub subdevice_send_offset: u16,
    /// SubDevice to MainDevice send mailbox size.
    #[wire(bytes = 2)]
    pub subdevice_send_size: u16,
    /// Mailbox protocols supported by the SubDevice.
    #[wire(bytes = 2)]
    pub supported_protocols: MailboxProtocols,
}

impl DefaultMailbox {
    pub fn has_mailbox(&self) -> bool {
        !self.supported_protocols.is_empty() && self.subdevice_receive_size > 0
            || self.subdevice_send_size > 0
    }
}

impl core::fmt::Debug for DefaultMailbox {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("MailboxConfig")
            .field(
                "subdevice_receive_offset",
                &format_args!("{:#06x}", self.subdevice_receive_offset),
            )
            .field(
                "subdevice_receive_size",
                &format_args!("{:#06x}", self.subdevice_receive_size),
            )
            .field(
                "subdevice_send_offset",
                &format_args!("{:#06x}", self.subdevice_send_offset),
            )
            .field(
                "subdevice_send_size",
                &format_args!("{:#06x}", self.subdevice_send_size),
            )
            .field("supported_protocols", &self.supported_protocols)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use crate::sync_manager_channel::Control;

    use super::*;
    use ethercrab_wire::EtherCrabWireWriteSized;

    #[test]
    fn sii_control_pack() {
        let ctl = SiiControl {
            access: SiiAccess::ReadWrite,
            emulate_sii: false,
            read_size: SiiReadSize::Octets8,
            address_type: SiiAddressSize::U8,
            read: false,
            write: false,
            reload: false,
            checksum_error: false,
            device_info_error: false,
            command_error: false,
            write_error: false,
            busy: true,
        };

        assert_eq!(ctl.pack(), [0b0100_0001, 0b1000_0000],);
    }

    #[test]
    fn sii_request_read_pack() {
        let packed = SiiRequest::read(0x1234).pack();

        assert_eq!(packed, [0x00, 0x01, 0x34, 0x12, 0x00, 0x00]);
    }

    #[test]
    fn sii_control_unpack() {
        let ctl = SiiControl {
            access: SiiAccess::ReadWrite,
            emulate_sii: false,
            read_size: SiiReadSize::Octets8,
            address_type: SiiAddressSize::U8,
            read: false,
            write: false,
            reload: false,
            checksum_error: false,
            device_info_error: false,
            command_error: false,
            write_error: false,
            busy: true,
        };

        assert_eq!(
            SiiControl::unpack_from_slice(&[0b0100_0001, 0b1000_0000]),
            Ok(ctl)
        );
    }

    #[test]
    fn sii_request_read_unpack() {
        let packed = SiiRequest::read(0x1234);

        let buf = [0x00, 0x01, 0x34, 0x12, 0x00, 0x00];

        assert_eq!(SiiRequest::unpack_from_slice(&buf), Ok(packed));
    }

    #[test]
    fn sii_general_ek1100() {
        let expected = SiiGeneral {
            group_string_idx: 2,
            image_string_idx: 0,
            order_string_idx: 1,
            name_string_idx: 4,
            coe_details: CoeDetails::empty(),
            foe_enabled: false,
            eoe_enabled: false,
            flags: Flags::empty(),
            ebus_current: -2000,
            ports: PortStatuses([
                PortStatus::Ebus,
                PortStatus::Unused,
                PortStatus::Unused,
                PortStatus::Unused,
            ]),
            physical_memory_addr: 0,
        };

        let raw = [2u8, 0, 1, 4, 2, 0, 0, 0, 0, 0, 0, 0, 48, 248, 3, 0, 0, 0];

        assert_eq!(SiiGeneral::unpack_from_slice(&raw), Ok(expected))
    }

    #[test]
    fn fmmu_ex() {
        let data = [0xaa, 0xbb, 0xcc];

        assert_eq!(
            FmmuEx::unpack_from_slice(&data),
            Ok(FmmuEx { sync_manager: 0xbb })
        );
    }

    #[test]
    fn recover_unknown_sm_types() {
        assert_eq!(
            SyncManager {
                start_addr: 0x1000,
                length: 0x0080,
                control: Control {
                    operation_mode: OperationMode::Mailbox,
                    direction: Direction::MasterWrite,
                    ecat_event_enable: true,
                    dls_user_event_enable: true,
                    watchdog_enable: false,
                },
                enable: SyncManagerEnable::ENABLE,
                usage_type: SyncManagerType::Unknown,
            }
            .usage_type(),
            SyncManagerType::MailboxWrite
        );

        assert_eq!(
            SyncManager {
                start_addr: 0x10c0,
                length: 0x0080,
                control: Control {
                    operation_mode: OperationMode::Mailbox,
                    direction: Direction::MasterRead,
                    ecat_event_enable: true,
                    dls_user_event_enable: true,
                    watchdog_enable: false,
                },
                enable: SyncManagerEnable::ENABLE,
                usage_type: SyncManagerType::Unknown,
            }
            .usage_type(),
            SyncManagerType::MailboxRead
        );

        assert_eq!(
            SyncManager {
                start_addr: 0x1180,
                length: 0x0006,
                control: Control {
                    operation_mode: OperationMode::Normal,
                    direction: Direction::MasterWrite,
                    ecat_event_enable: false,
                    dls_user_event_enable: true,
                    watchdog_enable: false,
                },
                enable: SyncManagerEnable::ENABLE,
                usage_type: SyncManagerType::Unknown,
            }
            .usage_type(),
            SyncManagerType::ProcessDataWrite
        );

        assert_eq!(
            SyncManager {
                start_addr: 0x1480,
                length: 0x0006,
                control: Control {
                    operation_mode: OperationMode::Normal,
                    direction: Direction::MasterRead,
                    ecat_event_enable: false,
                    dls_user_event_enable: false,
                    watchdog_enable: false,
                },
                enable: SyncManagerEnable::ENABLE,
                usage_type: SyncManagerType::Unknown,
            }
            .usage_type(),
            SyncManagerType::ProcessDataRead
        );
    }
}
