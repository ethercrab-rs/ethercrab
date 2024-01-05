use crate::{
    eeprom::types::{MailboxProtocols, SyncManagerType},
    pdi::PdiSegment,
};
use core::fmt::{self, Debug};

/// Slave identity information (vendor ID, product ID, etc).
#[derive(Default, Copy, Clone, PartialEq, ethercrab_wire::EtherCrabWireRead)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[wire(bytes = 16)]
pub struct SlaveIdentity {
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

impl fmt::Display for SlaveIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_fmt(format_args!(
            "vendor: {:#010x}, product {:#010x}, rev {}, serial {}",
            self.vendor_id, self.product_id, self.revision, self.serial
        ))
    }
}

impl Debug for SlaveIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SlaveIdentity")
            .field("vendor_id", &format_args!("{:#010x}", self.vendor_id))
            .field("product_id", &format_args!("{:#010x}", self.product_id))
            .field("revision", &self.revision)
            .field("serial", &self.serial)
            .finish()
    }
}

// impl FromEeprom for SlaveIdentity {
//     const STORAGE_SIZE: usize = 16;

//     fn parse_fields(i: &[u8]) -> IResult<&[u8], Self> {
//         let (i, vendor_id) = le_u32(i)?;
//         let (i, product_id) = le_u32(i)?;
//         let (i, revision) = le_u32(i)?;
//         let (i, serial) = le_u32(i)?;

//         all_consumed(i)?;

//         Ok((
//             i,
//             Self {
//                 vendor_id,
//                 product_id,
//                 revision,
//                 serial,
//             },
//         ))
//     }
// }

#[derive(Debug, Default, Clone, PartialEq)]
pub struct SlaveConfig {
    pub io: IoRanges,
    pub mailbox: MailboxConfig,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct MailboxConfig {
    pub(in crate::slave) read: Option<Mailbox>,
    pub(in crate::slave) write: Option<Mailbox>,
    pub(in crate::slave) supported_protocols: MailboxProtocols,
    pub(in crate::slave) coe_sync_manager_types: heapless::Vec<SyncManagerType, 16>,
    pub(in crate::slave) has_coe: bool,
    /// True if Complete Access is supported.
    pub(in crate::slave) complete_access: bool,
}

#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct Mailbox {
    pub(in crate::slave) address: u16,
    pub(in crate::slave) len: u16,
    pub(in crate::slave) sync_manager: u8,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct IoRanges {
    pub input: PdiSegment,
    pub output: PdiSegment,
}
