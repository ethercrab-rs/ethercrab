use core::fmt;

use num_enum::TryFromPrimitiveError;

use crate::PduData;

/// AL Status.
///
/// Defined in ETG1000.6 6.4.1
#[derive(Debug, Copy, Clone, num_enum::TryFromPrimitive, num_enum::IntoPrimitive)]
#[repr(u8)]
pub enum AlStatus {
    None = 0x00,
    Init = 0x01,
    PreOp = 0x02,
    Bootstrap = 0x03,
    SafeOp = 0x04,
    Op = 0x8,
}

impl fmt::Display for AlStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            AlStatus::None => "None",
            AlStatus::Init => "Init",
            AlStatus::PreOp => "Pre-Operational",
            AlStatus::Bootstrap => "Bootstrap",
            AlStatus::SafeOp => "Safe-Operational",
            AlStatus::Op => "Operational",
        };

        f.write_str(s)
    }
}

impl PduData for AlStatus {
    const LEN: u16 = u8::LEN;

    type Error = TryFromPrimitiveError<Self>;

    fn try_from_slice(slice: &[u8]) -> Result<Self, Self::Error> {
        Self::try_from(slice[0])
    }

    fn as_slice(&self) -> &[u8] {
        // SAFETY: Copied from `safe-transmute` crate so I'm assuming...
        // SAFETY: EtherCAT is little-endian on the wire, so this will ONLY work on
        // little-endian targets, hence the `compile_error!()` in `lib.rs`.
        unsafe {
            core::slice::from_raw_parts(
                u8::from(*self) as *const Self as *const u8,
                core::mem::size_of::<Self>(),
            )
        }
    }
}
