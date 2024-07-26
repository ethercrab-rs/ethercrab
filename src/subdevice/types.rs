use crate::{
    eeprom::types::{MailboxProtocols, SyncManagerType},
    pdi::PdiSegment,
};
use core::fmt::{self, Debug};

/// SubDevice identity information (vendor ID, product ID, etc).
#[derive(Default, Copy, Clone, PartialEq, ethercrab_wire::EtherCrabWireRead)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[wire(bytes = 16)]
pub struct SubDeviceIdentity {
    /// Vendor ID.
    #[wire(bytes = 4)]
    pub vendor_id: u32,
    /// Product ID.
    #[wire(bytes = 4)]
    pub product_id: u32,
    /// Product revision.
    #[wire(bytes = 4)]
    pub revision: u32,
    /// Device serial number.
    #[wire(bytes = 4)]
    pub serial: u32,
}

impl fmt::Display for SubDeviceIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_fmt(format_args!(
            "vendor: {:#010x}, product {:#010x}, rev {}, serial {}",
            self.vendor_id, self.product_id, self.revision, self.serial
        ))
    }
}

impl Debug for SubDeviceIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SubDeviceIdentity")
            .field("vendor_id", &format_args!("{:#010x}", self.vendor_id))
            .field("product_id", &format_args!("{:#010x}", self.product_id))
            .field("revision", &self.revision)
            .field("serial", &self.serial)
            .finish()
    }
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct SubDeviceConfig {
    pub io: IoRanges,
    pub mailbox: MailboxConfig,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct MailboxConfig {
    pub(in crate::subdevice) read: Option<Mailbox>,
    pub(in crate::subdevice) write: Option<Mailbox>,
    pub(in crate::subdevice) supported_protocols: MailboxProtocols,
    pub(in crate::subdevice) coe_sync_manager_types: heapless::Vec<SyncManagerType, 16>,
    pub(in crate::subdevice) has_coe: bool,
    /// True if Complete Access is supported.
    pub(in crate::subdevice) complete_access: bool,
}

#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct Mailbox {
    pub(in crate::subdevice) address: u16,
    pub(in crate::subdevice) len: u16,
    pub(in crate::subdevice) sync_manager: u8,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct IoRanges {
    pub input: PdiSegment,
    pub output: PdiSegment,
}
