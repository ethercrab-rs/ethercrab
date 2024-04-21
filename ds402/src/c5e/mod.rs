//! Nanotec C5-E.

mod control;
mod power;
mod status;

pub use control::ControlWord;
use ethercrab::{EtherCrabWireRead, EtherCrabWireWrite, Slave, SlavePdi, SlaveRef};
pub use power::Ds402State;
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

#[derive(ethercrab::EtherCrabWireWrite, Debug, Default)]
#[wire(bytes = 6)]
pub struct C5Outputs {
    #[wire(bytes = 2)]
    pub control: ControlWord,

    // FIXME: Should be target position
    #[wire(bytes = 4)]
    pub target_velocity: i32,
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

pub struct C5e<'sd> {
    sd: SlaveRef<'sd, SlavePdi<'sd>>,
    prev_state: Ds402State,
    outputs: C5Outputs,
}

impl<'sd> C5e<'sd> {
    pub fn new(sd: SlaveRef<'sd, SlavePdi<'sd>>) -> Self {
        Self {
            sd,
            prev_state: Ds402State::NotReadyToSwitchOn,
            outputs: C5Outputs::default(),
        }
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
        // TODO: This should be configurable at some point, but we'll just leave it in CSP for now
        // Opmode - Cyclic Synchronous Position
        // sd.sdo_write(0x6060, 0, 0x08u8).await?;
        // Opmode - Cyclic Synchronous Velocity
        sd.sdo_write(0x6060, 0, 0x09u8).await?;

        Ok(())
    }

    pub fn set_velocity(&mut self, velocity: i32) {
        self.outputs.target_velocity = velocity;
    }

    /// Extract DS402 status word from PDI.
    fn state(&self) -> Result<StatusWord, ethercrab::error::Error> {
        let pdi = C5Inputs::unpack_from_slice(self.sd.inputs_raw())?;

        Ok(pdi.status)
    }

    /// Update PDI with new output data.
    pub fn update_outputs(&mut self) {
        self.outputs
            .pack_to_slice_unchecked(self.sd.outputs_raw_mut());
    }

    pub fn current_state(&self) -> Result<Ds402State, ethercrab::error::Error> {
        self.state().map(|s| s.state())
    }

    pub fn state_change(&mut self) -> Option<(Ds402State, Ds402State)> {
        let new_state = self.current_state().ok()?;

        if new_state != self.prev_state {
            let prev_state = self.prev_state;

            self.prev_state = new_state;

            Some((prev_state, new_state))
        } else {
            None
        }
    }

    /// Reset state machine and clear fault.
    ///
    /// This is infallible because any prior state can enter fault mode.
    pub fn clear_fault(&mut self) {
        self.outputs.control = ControlWord {
            fault_reset: true,
            ..ControlWord::default()
        };
    }

    // TODO: Custom error type
    pub fn shutdown(&mut self) -> Result<(), ()> {
        let current_state = self.state().map_err(|_| ())?.state();

        if current_state != Ds402State::SwitchOnDisabled
            && current_state != Ds402State::NotReadyToSwitchOn
        {
            return Err(());
        }

        self.outputs.control = ControlWord {
            quick_stop: true,
            enable_voltage: true,
            ..ControlWord::default()
        };

        Ok(())
    }

    /// Transition from ready to switch on to switched on.
    // TODO: Custom error type
    pub fn switch_on(&mut self) -> Result<(), ()> {
        let current_state = self.state().map_err(|_| ())?.state();

        if current_state != Ds402State::ReadyToSwitchOn {
            return Err(());
        }

        self.outputs.control = ControlWord {
            quick_stop: true,
            switch_on: true,
            enable_voltage: true,
            ..ControlWord::default()
        };

        Ok(())
    }

    /// Transition from switched on to op
    // TODO: Custom error type
    pub fn enable_op(&mut self) -> Result<(), ()> {
        let current_state = self.state().map_err(|_| ())?.state();

        if current_state != Ds402State::SwitchedOn {
            return Err(());
        }

        self.outputs.control = ControlWord {
            switch_on: true,
            enable_voltage: true,
            enable_op: true,
            quick_stop: true,
            ..ControlWord::default()
        };

        Ok(())
    }
}
