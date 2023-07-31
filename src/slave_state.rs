use core::fmt;
use packed_struct::prelude::*;

/// AL (application layer) device state.
///
/// Defined in ETG1000.6 6.4.1
#[derive(
    Debug,
    Default,
    Copy,
    Clone,
    PartialEq,
    Eq,
    PrimitiveEnum,
    num_enum::TryFromPrimitive,
    num_enum::IntoPrimitive,
)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[repr(u8)]
pub enum SlaveState {
    /// No state recorded/read/known.
    #[default]
    None = 0x00,
    /// EtherCAT `INIT` state.
    Init = 0x01,
    /// EtherCAT `PRE-OP` state.
    PreOp = 0x02,
    /// EtherCAT `BOOT` state.
    Bootstrap = 0x03,
    /// EtherCAT `SAFE-OP` state.
    SafeOp = 0x04,
    /// EtherCAT `OP` state.
    Op = 0x8,
    /// State is unknown.
    Unknown = 0xff,
}

impl fmt::Display for SlaveState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            SlaveState::None => "None",
            SlaveState::Init => "Init",
            SlaveState::PreOp => "Pre-Operational",
            SlaveState::Bootstrap => "Bootstrap",
            SlaveState::SafeOp => "Safe-Operational",
            SlaveState::Op => "Operational",
            SlaveState::Unknown => "Unknown",
        };

        f.write_str(s)
    }
}
