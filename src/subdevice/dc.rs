//! Distributed Clock configuration for a single SubDevice.

use core::{fmt, time::Duration};

/// DC sync configuration for a SubDevice.
#[derive(Debug, Default, Copy, Clone, PartialEq, Eq)]
pub enum DcSync {
    /// DC sync is disabled for this SubDevice.
    #[default]
    Disabled,

    /// This SubDevice synchronises on the SYNC0 pulse.
    Sync0,

    /// Both SYNC0 and SYNC1 are enabled.
    ///
    /// SubDevices with an `AssignActivate` value of `0x0700` in their ESI definition should set
    /// this value as well as [`sync0_period`](crate::subdevice_group::DcConfiguration::sync0_period) in
    /// the SubDevice group DC configuration.
    Sync01 {
        /// SYNC1 cycle time.
        sync1_period: Duration,
    },
}

impl fmt::Display for DcSync {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DcSync::Disabled => f.write_str("disabled"),
            DcSync::Sync0 => f.write_str("SYNC0"),
            DcSync::Sync01 { sync1_period } => {
                write!(f, "SYNC0 with SYNC1 period {} us", sync1_period.as_micros())
            }
        }
    }
}
