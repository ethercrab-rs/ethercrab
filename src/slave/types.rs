use crate::{
    all_consumed,
    eeprom::types::{FromEeprom, MailboxProtocols, SyncManagerType},
    pdi::PdiSegment,
};
use core::fmt::{self, Debug};
use nom::{number::complete::le_u32, IResult};

#[derive(Default, Copy, Clone, PartialEq)]
pub struct SlaveIdentity {
    pub vendor_id: u32,
    pub product_id: u32,
    pub revision: u32,
    pub serial: u32,
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

impl FromEeprom for SlaveIdentity {
    const STORAGE_SIZE: usize = 16;

    fn parse_fields(i: &[u8]) -> IResult<&[u8], Self> {
        let (i, vendor_id) = le_u32(i)?;
        let (i, product_id) = le_u32(i)?;
        let (i, revision) = le_u32(i)?;
        let (i, serial) = le_u32(i)?;

        all_consumed(i)?;

        Ok((
            i,
            Self {
                vendor_id,
                product_id,
                revision,
                serial,
            },
        ))
    }
}

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

impl IoRanges {
    /// Expected working counter value for this slave.
    ///
    /// The working counter is calculated as follows:
    ///
    /// - If the slave has input data, increment by 1
    /// - If the slave has output data, increment by 2
    pub(crate) fn working_counter_sum(&self) -> u16 {
        let l = self.input.len().min(1) + (self.output.len().min(1) * 2);

        l as u16
    }
}
