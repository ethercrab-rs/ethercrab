//! DS402 state machine.

use core::{cell::UnsafeCell, fmt};
use std::thread::current;

use crate::{error::Error as EthercrabError, GroupSlave};

smlang::statemachine! {
    transitions: {
        *NotReadyToSwitchOn + EnableOp [ clear_faults ] = SwitchOnDisabled,
        SwitchOnDisabled + EnableOp [ shutdown ] = ReadyToSwitchOn,
        ReadyToSwitchOn + EnableOp [ switch_on ] = SwitchedOn,
        SwitchedOn + EnableOp [ enable_op ] = OpEnable,

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
        self.set_control_word(ControlWord::STATE_SHUTDOWN);

        self.status_word()
            .mandatory()
            .eq(&StatusWord::STATE_READY_TO_SWITCH_ON)
            .then_some(())
            .ok_or(())
    }

    fn switch_on(&mut self) -> Result<(), ()> {
        self.set_control_word(ControlWord::STATE_SWITCH_ON);

        self.status_word()
            .mandatory()
            .eq(&StatusWord::STATE_SWITCHED_ON)
            .then_some(())
            .ok_or(())
    }

    fn enable_op(&mut self) -> Result<(), ()> {
        self.set_control_word(ControlWord::STATE_ENABLE_OP);

        self.status_word()
            .mandatory()
            .eq(&StatusWord::STATE_OP_ENABLE)
            .then_some(())
            .ok_or(())
    }

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

pub struct Ds402<'a> {
    pub slave: GroupSlave<'a>,
}

impl<'a> Ds402<'a> {
    pub fn new(slave: GroupSlave<'a>) -> Result<Self, EthercrabError> {
        Ok(Self { slave })
    }

    pub fn status_word(&self) -> StatusWord {
        let status = u16::from_le_bytes(self.slave.inputs()[0..=1].try_into().unwrap());

        unsafe { StatusWord::from_bits_unchecked(status) }
    }

    fn set_control_word(&mut self, state: ControlWord) {
        let (control, rest) = self.slave.outputs().split_at_mut(2);

        let state = state.bits.to_le_bytes();

        control.copy_from_slice(&state);
    }
}

pub struct Ds402Sm<'a> {
    // TODO: Un-pub
    pub sm: StateMachine<Ds402<'a>>,
    prev_status: StatusWord,
}

impl<'a> Ds402Sm<'a> {
    pub fn is_op(&self) -> bool {
        self.sm.state == States::OpEnable
    }

    pub fn new(context: Ds402<'a>) -> Self {
        Self {
            sm: StateMachine::new(context),
            prev_status: StatusWord::empty(),
        }
    }

    // DELETEME
    pub fn io(&self) -> (&[u8], &mut [u8]) {
        (
            self.sm.context.slave.inputs(),
            self.sm.context.slave.outputs(),
        )
    }

    pub fn status_word(&self) -> StatusWord {
        self.sm.context.status_word()
    }

    /// Returns a "ready for cyclic IO" flag.
    pub fn tick(&mut self) -> bool {
        let status = self.sm.context().status_word();

        if let Ok(_) = self.sm.process_event(Events::EnableOp) {
            log::debug!("Edge {:?} -> {:?}", self.prev_status, status);
        }

        self.sm.state == States::OpEnable
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

        const STATE_SHUTDOWN = Self::QUICK_STOP.bits | Self::DISABLE_VOLTAGE.bits;
        const STATE_SWITCH_ON = Self::QUICK_STOP.bits | Self::DISABLE_VOLTAGE.bits | Self::SWITCH_ON.bits;
        const STATE_DISABLE_VOLTAGE = 0;
        const STATE_QUICK_STOP = Self::DISABLE_VOLTAGE.bits;
        const STATE_DISABLE_OP = Self::QUICK_STOP.bits | Self::DISABLE_VOLTAGE.bits | Self::SWITCH_ON.bits;
        const STATE_ENABLE_OP = Self::ENABLE_OP.bits | Self::QUICK_STOP.bits | Self::DISABLE_VOLTAGE.bits | Self::SWITCH_ON.bits;
        const STATE_FAULT_RESET = Self::RESET_FAULT.bits;
    }
}

bitflags::bitflags! {
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
        Self::READY_TO_SWITCH_ON.bits
            | Self::SWITCHED_ON.bits
            | Self::OP_ENABLED.bits
            | Self::FAULT.bits
            | Self::SWITCH_ON_DISABLED.bits,
    );

    const STATE_NOT_READY_TO_SWITCH_ON: Self = Self::empty();
    const STATE_SWITCH_ON_DISABLED: Self = Self::from_bits_truncate(Self::SWITCH_ON_DISABLED.bits);
    const STATE_READY_TO_SWITCH_ON: Self = Self::from_bits_truncate(Self::READY_TO_SWITCH_ON.bits);
    const STATE_SWITCHED_ON: Self =
        Self::from_bits_truncate(Self::READY_TO_SWITCH_ON.bits | Self::SWITCHED_ON.bits);
    const STATE_OP_ENABLE: Self = Self::from_bits_truncate(
        Self::OP_ENABLED.bits | Self::READY_TO_SWITCH_ON.bits | Self::SWITCHED_ON.bits,
    );
    const STATE_FAULT_REACTION_ACTIVE: Self =
        Self::from_bits_truncate(Self::STATE_OP_ENABLE.bits | Self::FAULT.bits);

    fn mandatory(self) -> Self {
        self.intersection(Self::MANDATORY)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check() {
        env_logger::try_init().ok();

        let inputs = [0x00, 0x00];
        let mut outputs = [0x00, 0x00];

        // let mut sm = Ds402::new(&inputs, &mut outputs);

        // while !sm.is_op() {
        //     sm.tick();
        // }
    }

    #[test]
    fn ignored_quick_stop() {
        let bits = 0b100000u16;

        assert_eq!(
            unsafe { StatusWord::from_bits_unchecked(bits) }.is_not_ready_to_switch_on(),
            true
        );

        let bits = 0u16;

        assert_eq!(
            unsafe { StatusWord::from_bits_unchecked(bits) }.is_not_ready_to_switch_on(),
            true
        );
    }
}
