use crate::{fmt, pdu_data::PduRead};
use num_enum::TryFromPrimitive;
use packed_struct::prelude::*;

/// AL (application layer) state for a single device.
///
/// Read from register `0x0130` ([`RegisterAddress::AlStatus`](crate::register::RegisterAddress::AlStatus)).
///
/// Defined in ETG1000.6 6.4.1, ETG1000.6 Table 9.
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
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
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

impl core::fmt::Display for SlaveState {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
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

impl PduRead for SlaveState {
    const LEN: u16 = u8::LEN;

    type Error = ();

    fn try_from_slice(slice: &[u8]) -> Result<Self, Self::Error> {
        Self::try_from_primitive(slice[0]).map_err(|e| {
            fmt::error!("Failed to decide SlaveState from number {:?}", e.number);

            ()
        })
    }
}

bitflags::bitflags! {
    /// AL (application layer) state for one or more devices.
    ///
    /// Read from register `0x0130` ([`RegisterAddress::AlStatus`](crate::register::RegisterAddress::AlStatus)).
    ///
    /// Defined in ETG1000.6 6.4.1, ETG1000.6 Table 9.
    #[derive(
        Debug,
        Default,
        Copy,
        Clone,
        PartialEq,
        Eq,
    )]

    pub struct SlaveStates: u8 {
        /// No state recorded/read/known.
        const NONE = 0x00;
        /// EtherCAT `INIT` state.
        const INIT = 0x01;
        /// EtherCAT `PRE-OP` state.
        const PRE_OP = 0x02;
        /// EtherCAT `SAFE-OP` state.
        const SAFE_OP = 0x04;
        /// EtherCAT `OP` state.
        const OP = 0x8;
    }
}

#[cfg(feature = "defmt")]
impl defmt::Format for SlaveStates {
    fn format(&self, fmt: defmt::Formatter) {
        defmt::write!(fmt, "SlaveStates({:02x})", self.bits());
    }
}

impl PduRead for SlaveStates {
    const LEN: u16 = u8::LEN;

    type Error = core::convert::Infallible;

    fn try_from_slice(slice: &[u8]) -> Result<Self, Self::Error> {
        let res = Self::from_bits_truncate(slice[0]);

        Ok(res)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_status() {
        let expected = SlaveStates::PRE_OP | SlaveStates::SAFE_OP;

        let res = SlaveStates::try_from_slice(&[0x02 | 0x04]);

        assert_eq!(res, Ok(expected));
    }

    #[test]
    fn unknown_status() {
        let expected = SlaveStates::NONE;

        let res = SlaveStates::try_from_slice(&[0xf0]);

        assert_eq!(res, Ok(expected));
    }

    #[test]
    fn unknown_status_and_op() {
        let expected = SlaveStates::OP;

        let res = SlaveStates::try_from_slice(&[0x08 | 0xf0]);

        assert_eq!(res, Ok(expected));
    }
}
