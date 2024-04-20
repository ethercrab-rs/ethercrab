//! Nanotec C5-E.

mod control;
mod status;

pub use control::ControlWord;
use ethercrab::{Slave, SlavePdi, SlaveRef};
pub use status::StatusWord;
use std::ops::Deref;

/// C5-E manual page 127 "Error number"
#[derive(ethercrab::EtherCrabWireRead, Debug)]
#[wire(bytes = 4)]
pub struct C5Error {
    #[wire(bytes = 2)]
    pub code: u16,
    #[wire(bytes = 1)]
    pub class: u8,
    #[wire(bytes = 1)]
    pub number: u8,
}

#[derive(ethercrab::EtherCrabWireRead, Debug)]
#[wire(bytes = 10)]
pub struct C5Inputs {
    #[wire(bytes = 2)]
    pub status: StatusWord,

    #[wire(bytes = 4)]
    pub actual_position: i32,

    #[wire(bytes = 4)]
    pub actual_velocity: i32,
}

/// ETG6010 section 5.1 State Machine
#[derive(Debug, Copy, Clone)]
pub enum Ds402State {
    NotReadyToSwitchOn,
    SwitchOnDisabled,
    ReadyToSwitchOn,
    SwitchedOn,
    OpEnabled,
    QuickStop,
    FaultReact,
    Fault,
}

pub struct C5e<'sd> {
    sd: SlaveRef<'sd, SlavePdi<'sd>>,
}

impl<'sd> C5e<'sd> {
    pub fn new(sd: SlaveRef<'sd, SlavePdi<'sd>>) -> Self {
        Self { sd }
    }

    pub async fn configure<'a>(
        sd: &'a SlaveRef<'a, impl Deref<Target = Slave>>,
    ) -> Result<(), ethercrab::error::Error> {
        // TODO: Check identity

        // Manual section 4.8 Setting the motor data
        // 1.8deg step, so 50 pole pairs
        sd.sdo_write(0x2030, 0, 50u32).await?;
        // Max motor current in mA.
        sd.sdo_write(0x2031, 0, 1000u32).await?;
        // Rated motor current in mA
        sd.sdo_write(0x6075, 0, 2820u32).await?;
        // Max motor current, % of rated current in milli-percent, i.e. 1000 is 100%
        sd.sdo_write(0x6073, 0, 1000u16).await?;
        // Max motor current max duration in ms
        sd.sdo_write(0x203b, 02, 100u32).await?;
        // Motor type: stepper
        sd.sdo_write(0x3202, 00, 0x08u32).await?;
        // Test motor has 500ppr incremental encoder, differential
        sd.sdo_write(0x2059, 00, 0x0u32).await?;
        // Set velocity unit to RPM (factory default)
        sd.sdo_write(0x60a9, 00, 0x00B44700u32).await?;

        // CSV described a bit better in section 7.6.2.2 Related Objects of the manual
        sd.sdo_write(0x1600, 0, 0u8).await?;
        // Control word, u16
        // NOTE: The lower word specifies the field length
        sd.sdo_write(0x1600, 1, 0x6040_0010u32).await?;
        // Target velocity, i32
        sd.sdo_write(0x1600, 2, 0x60ff_0020u32).await?;
        sd.sdo_write(0x1600, 0, 2u8).await?;

        sd.sdo_write(0x1a00, 0, 0u8).await?;
        // Status word, u16
        sd.sdo_write(0x1a00, 1, 0x6041_0010u32).await?;
        // Actual position, i32
        sd.sdo_write(0x1a00, 2, 0x6064_0020u32).await?;
        // Actual velocity, i32
        sd.sdo_write(0x1a00, 3, 0x606c_0020u32).await?;
        sd.sdo_write(0x1a00, 0, 0x03u8).await?;

        sd.sdo_write(0x1c12, 0, 0u8).await?;
        sd.sdo_write(0x1c12, 1, 0x1600u16).await?;
        sd.sdo_write(0x1c12, 0, 1u8).await?;

        sd.sdo_write(0x1c13, 0, 0u8).await?;
        sd.sdo_write(0x1c13, 1, 0x1a00u16).await?;
        sd.sdo_write(0x1c13, 0, 1u8).await?;

        // FIXME: We want CSP!
        // Opmode - Cyclic Synchronous Position
        // sd.sdo_write(0x6060, 0, 0x08u8).await?;
        // Opmode - Cyclic Synchronous Velocity
        sd.sdo_write(0x6060, 0, 0x09u8).await?;

        Ok(())
    }
}
