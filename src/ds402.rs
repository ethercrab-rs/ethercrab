//! DS402 state machine.

use crate::{error::Error as EthercrabError, GroupSlave};
use core::fmt;

smlang::statemachine! {
    transitions: {
        *NotReadyToSwitchOn + EnableOp [ clear_faults ] = SwitchOnDisabled,
        SwitchOnDisabled + EnableOp [ shutdown ] = ReadyToSwitchOn,
        ReadyToSwitchOn + EnableOp [ switch_on ] = SwitchedOn,
        SwitchedOn + EnableOp [ enable_op ] = OpEnable,

        OpEnable + DisableOp [ disable_op ] = SwitchedOn,
        SwitchedOn + DisableOp [ switch_off ] = ReadyToSwitchOn,
        ReadyToSwitchOn + DisableOp [ disable_switch_on ] = SwitchOnDisabled,

        // TODO: Graceful shutdown
        OpEnable + Tick = QuickStopActive,
        QuickStopActive + Tick = OpEnable,
        FaultReactionActive + Tick = Fault,
        Fault + ResetFault = ResettingFault,
        ResettingFault + Tick [ clear_faults ] = SwitchOnDisabled,
        _ + FaultDetected = Fault,
    }
}

impl<'a> StateMachineContext for Ds402<'a> {
    fn shutdown(&mut self) -> Result<(), ()> {
        // self.set_control_word(ControlWord::STATE_SHUTDOWN);

        // self.status_word()
        //     .mandatory()
        //     .eq(&StatusWord::STATE_READY_TO_SWITCH_ON)
        //     .then_some(())
        //     .ok_or(())

        self.set_and_read(
            ControlWord::STATE_SHUTDOWN,
            StatusWord::STATE_READY_TO_SWITCH_ON,
        )
    }

    fn switch_on(&mut self) -> Result<(), ()> {
        // self.set_control_word(ControlWord::STATE_SWITCH_ON);

        // self.status_word()
        //     .mandatory()
        //     .eq(&StatusWord::STATE_SWITCHED_ON)
        //     .then_some(())
        //     .ok_or(())

        self.set_and_read(ControlWord::STATE_SWITCH_ON, StatusWord::STATE_SWITCHED_ON)
    }

    fn enable_op(&mut self) -> Result<(), ()> {
        // self.set_control_word(ControlWord::STATE_ENABLE_OP);

        // self.status_word()
        //     .mandatory()
        //     .eq(&StatusWord::STATE_OP_ENABLE)
        //     .then_some(())
        //     .ok_or(())

        self.set_and_read(ControlWord::STATE_ENABLE_OP, StatusWord::STATE_OP_ENABLE)
    }

    // ---

    fn disable_op(&mut self) -> Result<(), ()> {
        self.set_and_read(ControlWord::STATE_SWITCH_ON, StatusWord::STATE_SWITCHED_ON)
    }
    fn switch_off(&mut self) -> Result<(), ()> {
        self.set_and_read(
            ControlWord::STATE_SHUTDOWN,
            StatusWord::STATE_READY_TO_SWITCH_ON,
        )
    }
    fn disable_switch_on(&mut self) -> Result<(), ()> {
        self.set_and_read(
            ControlWord::STATE_DISABLE_VOLTAGE,
            StatusWord::STATE_SWITCH_ON_DISABLED,
        )
    }

    // ---

    fn clear_faults(&mut self) -> Result<(), ()> {
        self.set_control_word(ControlWord::STATE_FAULT_RESET);

        (!self.status_word().mandatory().contains(StatusWord::FAULT))
            .then_some(())
            .ok_or(())
    }
}

impl fmt::Debug for States {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Fault => write!(f, "Fault"),
            Self::FaultReactionActive => write!(f, "FaultReactionActive"),
            Self::NotReadyToSwitchOn => write!(f, "NotReadyToSwitchOn"),
            Self::OpEnable => write!(f, "OpEnable"),
            Self::QuickStopActive => write!(f, "QuickStopActive"),
            Self::ReadyToSwitchOn => write!(f, "ReadyToSwitchOn"),
            Self::ResettingFault => write!(f, "ResettingFault"),
            Self::SwitchOnDisabled => write!(f, "SwitchOnDisabled"),
            Self::SwitchedOn => write!(f, "SwitchedOn"),
        }
    }
}

impl Clone for States {
    fn clone(&self) -> Self {
        match self {
            Self::Fault => Self::Fault,
            Self::FaultReactionActive => Self::FaultReactionActive,
            Self::NotReadyToSwitchOn => Self::NotReadyToSwitchOn,
            Self::OpEnable => Self::OpEnable,
            Self::QuickStopActive => Self::QuickStopActive,
            Self::ReadyToSwitchOn => Self::ReadyToSwitchOn,
            Self::ResettingFault => Self::ResettingFault,
            Self::SwitchOnDisabled => Self::SwitchOnDisabled,
            Self::SwitchedOn => Self::SwitchedOn,
        }
    }
}

/// DS402/CiA402 wrapper around a single EtherCat slave.
#[derive(Debug)]
pub struct Ds402<'a> {
    /// The EtherCat slave.
    pub slave: GroupSlave<'a>,
}

impl<'a> Ds402<'a> {
    /// Create a new DS402 state machine.
    pub fn new(slave: GroupSlave<'a>) -> Result<Self, EthercrabError> {
        Ok(Self { slave })
    }

    fn set_and_read(&mut self, set: ControlWord, read: StatusWord) -> Result<(), ()> {
        self.set_control_word(set);

        self.status_word()
            .mandatory()
            .eq(&read)
            .then_some(())
            .ok_or(())
    }

    /// Get the DS402 status word.
    pub fn status_word(&self) -> StatusWord {
        let status = u16::from_le_bytes(self.slave.inputs()[0..=1].try_into().unwrap());

        StatusWord::from_bits_truncate(status)
    }

    fn set_control_word(&mut self, state: ControlWord) {
        let (control, _rest) = self.slave.outputs().split_at_mut(2);

        let state = state.bits().to_le_bytes();

        control.copy_from_slice(&state);
    }
}

/// DS402 state machine.
pub struct Ds402Sm<'a> {
    sm: StateMachine<Ds402<'a>>,
}

impl<'a> Ds402Sm<'a> {
    /// Returns true if the slave is in `OP` state.
    ///
    /// NOTE: Not to be confused with EtherCAT's `OP` state; that is a precondition for running the
    /// DS402 SM.
    pub fn is_op(&self) -> bool {
        self.sm.state == States::OpEnable
    }

    /// Create a new DS402 state machine with the given slave.
    pub fn new(context: Ds402<'a>) -> Self {
        Self {
            sm: StateMachine::new(context),
        }
    }

    /// Get a reference to the underlying EtherCAT slave device.
    pub fn slave(&self) -> &GroupSlave {
        &self.sm.context().slave
    }

    /// Get the DS402 status word.
    pub fn status_word(&self) -> StatusWord {
        self.sm.context.status_word()
    }

    /// Returns a "ready for cyclic IO" flag.
    pub fn tick(&mut self) -> bool {
        let status = self.sm.context().status_word();

        if self.sm.process_event(Events::EnableOp).is_ok() {
            log::debug!("Edge {:?}", status);
        }

        self.sm.state == States::OpEnable
    }

    /// Put the slave into "switch on disabled" state. Returns true when finished.
    // TODO: Some sort of typestate API to transition between higher level states so we can't do
    // this during normal op
    pub fn tick_shutdown(&mut self) -> bool {
        let status = self.sm.context().status_word();

        if self.sm.process_event(Events::DisableOp).is_ok() {
            log::debug!("Edge {:?}", status);
        }

        self.sm.state == States::SwitchOnDisabled
    }
}

bitflags::bitflags! {
    /// AKD EtherCAT Communications Manual section 5.3.55
    pub struct ControlWord: u16 {
        /// Switch on
        const SWITCH_ON = 1 << 0;
        /// Disable Voltage
        const DISABLE_VOLTAGE = 1 << 1;
        /// Quick Stop
        const QUICK_STOP = 1 << 2;
        /// Enable Operation
        const ENABLE_OP = 1 << 3;
        /// Operation mode specific
        const OP_SPECIFIC_1 = 1 << 4;
        /// Operation mode specific
        const OP_SPECIFIC_2 = 1 << 5;
        /// Operation mode specific
        const OP_SPECIFIC_3 = 1 << 6;
        /// Reset Fault (only effective for faults)
        const RESET_FAULT = 1 << 7;
        /// Pause/halt
        const PAUSE = 1 << 8;

        /// Shutdown state.
        const STATE_SHUTDOWN = Self::QUICK_STOP.bits() | Self::DISABLE_VOLTAGE.bits();
        /// Switched on state.
        const STATE_SWITCH_ON = Self::QUICK_STOP.bits() | Self::DISABLE_VOLTAGE.bits() | Self::SWITCH_ON.bits();
        /// Voltage disabled state.
        const STATE_DISABLE_VOLTAGE = 0;
        /// Quick stop state.
        const STATE_QUICK_STOP = Self::DISABLE_VOLTAGE.bits();
        /// Operation disabled state.
        const STATE_DISABLE_OP = Self::QUICK_STOP.bits() | Self::DISABLE_VOLTAGE.bits() | Self::SWITCH_ON.bits();
        /// Operation enabled state.
        const STATE_ENABLE_OP = Self::ENABLE_OP.bits() | Self::QUICK_STOP.bits() | Self::DISABLE_VOLTAGE.bits() | Self::SWITCH_ON.bits();
        /// Fault reset state.
        const STATE_FAULT_RESET = Self::RESET_FAULT.bits();
    }
}

bitflags::bitflags! {
    #[derive(Debug, Copy, Clone, PartialEq, Eq)]
    /// AKD EtherCAT Communications Manual section   5.3.56
    pub struct StatusWord: u16 {
        /// Ready to switch on
        const READY_TO_SWITCH_ON = 1 << 0;
        /// Switched on
        const SWITCHED_ON = 1 << 1;
        /// Operation enabled
        const OP_ENABLED = 1 << 2;
        /// Fault
        const FAULT = 1 << 3;
        /// Voltage enabled
        const VOLTAGE_ENABLED = 1 << 4;
        /// Quick stop
        const QUICK_STOP = 1 << 5;
        /// Switch on disabled
        const SWITCH_ON_DISABLED = 1 << 6;
        /// Warning
        const WARNING = 1 << 7;
        /// STO – Safe Torque Off
        const STO = 1 << 8;
        /// Remote
        const REMOTE = 1 << 9;
        /// Target reached
        const TARGET_REACHED = 1 << 10;
        /// Internal limit active
        const INTERNAL_LIMIT = 1 << 11;
        /// Operation mode specific (reserved)
        const OP_SPECIFIC_1 = 1 << 12;
        /// Operation mode specific (reserved)
        const OP_SPECIFIC_2 = 1 << 13;
        /// Manufacturer-specific (reserved)
        const MAN_SPECIFIC_1 = 1 << 14;
        /// Manufacturer-specific (reserved)
        const MAN_SPECIFIC_2 = 1 << 15;
    }
}

impl StatusWord {
    /// Mandatory status bits as per ETG6010 section 5.3.
    const MANDATORY: Self = Self::from_bits_truncate(
        Self::READY_TO_SWITCH_ON.bits()
            | Self::SWITCHED_ON.bits()
            | Self::OP_ENABLED.bits()
            | Self::FAULT.bits()
            | Self::SWITCH_ON_DISABLED.bits(),
    );

    // const STATE_NOT_READY_TO_SWITCH_ON: Self = Self::empty();
    const STATE_SWITCH_ON_DISABLED: Self =
        Self::from_bits_truncate(Self::SWITCH_ON_DISABLED.bits());
    const STATE_READY_TO_SWITCH_ON: Self =
        Self::from_bits_truncate(Self::READY_TO_SWITCH_ON.bits());
    const STATE_SWITCHED_ON: Self =
        Self::from_bits_truncate(Self::READY_TO_SWITCH_ON.bits() | Self::SWITCHED_ON.bits());
    const STATE_OP_ENABLE: Self = Self::from_bits_truncate(
        Self::OP_ENABLED.bits() | Self::READY_TO_SWITCH_ON.bits() | Self::SWITCHED_ON.bits(),
    );
    // const STATE_FAULT_REACTION_ACTIVE: Self =
    //     Self::from_bits_truncate(Self::STATE_OP_ENABLE.bits() | Self::FAULT.bits());

    fn mandatory(self) -> Self {
        self.intersection(Self::MANDATORY)
    }
}
