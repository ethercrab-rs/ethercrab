/// AL (application layer) state for a single SubDevice.
///
/// Read from register `0x0130` ([`RegisterAddress::AlStatus`](crate::register::RegisterAddress::AlStatus)).
///
/// Defined in ETG1000.6 6.4.1, ETG1000.6 Table 9.
#[derive(Debug, Copy, Clone, PartialEq, Eq, ethercrab_wire::EtherCrabWireReadWrite)]
#[doc(alias = "SlaveState")]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[repr(u8)]
pub enum SubDeviceState {
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
    #[wire(catch_all)]
    Other(u8),
}

impl Default for SubDeviceState {
    fn default() -> Self {
        Self::None
    }
}

impl core::fmt::Display for SubDeviceState {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            SubDeviceState::None => f.write_str("None"),
            SubDeviceState::Init => f.write_str("Init"),
            SubDeviceState::PreOp => f.write_str("Pre-Operational"),
            SubDeviceState::Bootstrap => f.write_str("Bootstrap"),
            SubDeviceState::SafeOp => f.write_str("Safe-Operational"),
            SubDeviceState::Op => f.write_str("Operational"),
            SubDeviceState::Other(value) => write!(f, "Other({:01x})", value),
        }
    }
}
