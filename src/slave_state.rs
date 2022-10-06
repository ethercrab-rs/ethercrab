use core::fmt;
use packed_struct::prelude::*;

/// AL Status.
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
#[repr(u8)]
pub enum SlaveState {
    #[default]
    None = 0x00,
    Init = 0x01,
    PreOp = 0x02,
    Bootstrap = 0x03,
    SafeOp = 0x04,
    Op = 0x8,
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
