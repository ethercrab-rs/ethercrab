use crate::pdu_data::PduRead;

/// AL (application layer) state for a single device.
///
/// Read from register `0x0130` ([`RegisterAddress::AlStatus`](crate::register::RegisterAddress::AlStatus)).
///
/// Defined in ETG1000.6 6.4.1, ETG1000.6 Table 9.
#[derive(Debug, Copy, Clone, PartialEq, Eq, num_enum::FromPrimitive, num_enum::IntoPrimitive)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[repr(u8)]
pub enum SlaveState {
    /// No state recorded/read/known.
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
    /// State is a combination of above variants or is an unknown value.
    #[num_enum(catch_all)]
    Other(u8),
}

impl Default for SlaveState {
    fn default() -> Self {
        Self::None
    }
}

impl core::fmt::Display for SlaveState {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            SlaveState::None => f.write_str("None"),
            SlaveState::Init => f.write_str("Init"),
            SlaveState::PreOp => f.write_str("Pre-Operational"),
            SlaveState::Bootstrap => f.write_str("Bootstrap"),
            SlaveState::SafeOp => f.write_str("Safe-Operational"),
            SlaveState::Op => f.write_str("Operational"),
            SlaveState::Other(value) => write!(f, "Other({:01x})", value),
        }
    }
}

impl PduRead for SlaveState {
    const LEN: u16 = u8::LEN;

    type Error = core::convert::Infallible;

    fn try_from_slice(slice: &[u8]) -> Result<Self, Self::Error> {
        Ok(Self::from(slice[0]))
    }
}
