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
    PrimitiveEnum,
    num_enum::TryFromPrimitive,
    num_enum::IntoPrimitive,
)]
#[repr(u8)]
pub enum AlState {
    #[default]
    None = 0x00,
    Init = 0x01,
    PreOp = 0x02,
    Bootstrap = 0x03,
    SafeOp = 0x04,
    Op = 0x8,
}

impl fmt::Display for AlState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            AlState::None => "None",
            AlState::Init => "Init",
            AlState::PreOp => "Pre-Operational",
            AlState::Bootstrap => "Bootstrap",
            AlState::SafeOp => "Safe-Operational",
            AlState::Op => "Operational",
        };

        f.write_str(s)
    }
}
