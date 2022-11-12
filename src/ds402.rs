//! DS402 state machine.

use core::fmt;
use std::thread::current;

use crate::{error::Error as EthercrabError, GroupSlave};

// smlang::statemachine! {
//     transitions: {
//         *Start + Tick [ switch_on_is_disabled ] = NotReadyToSwitchOn,
//         NotReadyToSwitchOn + Tick  = SwitchOnDisabled,
//         SwitchOnDisabled + Tick  = ReadyToSwitchOn,
//         ReadyToSwitchOn + Tick = SwitchedOn,
//         SwitchedOn + Tick = OpEnable,
//         OpEnable + Tick = QuickStopActive,
//         QuickStopActive + Tick = OpEnable,
//         FaultReactionActive + Tick = Fault,
//         Fault + Tick = ResettingFault,
//         _ + FaultDetected = Fault,
//     }
// }

// impl<'a> StateMachineContext for Ds402<'a> {
//     // fn reset_fault(&mut self) {
//     //     self.set_control_word(ControlWord::RESET_FAULT);
//     // }

//     // fn disable_switch_on(&mut self) {
//     //     self.set_control_word(ControlWord::SWITCH_ON_DISABLED);
//     // }

//     // // ---

//     // fn faults_cleared(&mut self) -> Result<(), ()> {
//     //     (!self.status().contains(StatusWord::FAULT))
//     //         .then_some(())
//     //         .ok_or(())
//     // }

//     // fn is_not_ready_to_switch_on(&mut self) -> Result<(), ()> {
//     //     self.status().is_empty().then_some(()).ok_or(())
//     // }

//     fn has_fault(&mut self) -> Result<(), ()> {
//         self.status()
//             .contains(&StatusWord::FAULT)
//             .then_some(())
//             .ok_or(())
//     }

//     fn is_switch_on_disabled(&mut self) -> Result<(), ()> {
//         self.status()
//             .without_quickstop()
//             .eq(&StatusWord::SWITCH_ON_DISABLED)
//             .then_some(())
//             .ok_or(())
//     }

//        fn is_ready_to_switch_on(&mut self) -> Result<(), ()> {
//         self.status()

//             .eq(&StatusWord::QUICK_STOP | StatusWord::)
//             .then_some(())
//             .ok_or(())
//     }
// }

// impl fmt::Debug for States {
//     fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
//         match self {
//             Self::Fault => write!(f, "Fault"),
//             Self::FaultReactionActive => write!(f, "FaultReactionActive"),
//             Self::NotReadyToSwitchOn => write!(f, "NotReadyToSwitchOn"),
//             Self::OpEnable => write!(f, "OpEnable"),
//             Self::QuickStopActive => write!(f, "QuickStopActive"),
//             Self::ReadyToSwitchOn => write!(f, "ReadyToSwitchOn"),
//             Self::ResettingFault => write!(f, "ResettingFault"),
//             Self::Start => write!(f, "Start"),
//             Self::SwitchOnDisabled => write!(f, "SwitchOnDisabled"),
//             Self::SwitchedOn => write!(f, "SwitchedOn"),
//         }
//     }
// }

// impl Clone for States {
//     fn clone(&self) -> Self {
//         match self {
//             Self::Fault => Self::Fault,
//             Self::FaultReactionActive => Self::FaultReactionActive,
//             Self::NotReadyToSwitchOn => Self::NotReadyToSwitchOn,
//             Self::OpEnable => Self::OpEnable,
//             Self::QuickStopActive => Self::QuickStopActive,
//             Self::ReadyToSwitchOn => Self::ReadyToSwitchOn,
//             Self::ResettingFault => Self::ResettingFault,
//             Self::Start => Self::Start,
//             Self::SwitchOnDisabled => Self::SwitchOnDisabled,
//             Self::SwitchedOn => Self::SwitchedOn,
//         }
//     }
// }

pub struct Ds402<'a> {
    // TODO: Un-pub
    pub inputs: &'a [u8],
    pub outputs: &'a mut [u8],
    pub state: State,
}

impl<'a> Ds402<'a> {
    // TODO: Make this unit testable by decoupling GroupSlave
    pub fn new<TIMEOUT>(slave: &'a mut GroupSlave<'a, TIMEOUT>) -> Result<Self, EthercrabError> {
        let inputs = slave.inputs.as_ref().ok_or(EthercrabError::Internal)?;
        let outputs = slave.outputs.as_mut().ok_or(EthercrabError::Internal)?;

        Ok(Self {
            inputs,
            outputs,
            state: State::NotReadyToSwitchOn,
        })
    }

    pub fn status_word(&self) -> StatusWord {
        let status = u16::from_le_bytes(self.inputs[0..=1].try_into().unwrap());

        unsafe { StatusWord::from_bits_unchecked(status) }
    }

    fn set_control_word(&mut self, state: ControlWord) {
        let (control, rest) = self.outputs.split_at_mut(2);

        let state = state.bits.to_le_bytes();

        control.copy_from_slice(&state);
    }

    pub fn tick(&mut self) -> Option<State> {
        let next_state = self.status_word().as_state();
        log::debug!(
            "Tick {:?} {:?} {:?}",
            self.state,
            next_state,
            self.status_word()
        );

        let should_transition = match (self.state, next_state) {
            (_, State::FaultReactionActive) => true,
            (_, State::Fault) => true,
            // Fault has been reset
            (State::Fault, State::SwitchOnDisabled) => true,
            (State::NotReadyToSwitchOn, State::SwitchOnDisabled) => true,
            (State::SwitchOnDisabled, State::ReadyToSwitchOn) => true,
            (State::ReadyToSwitchOn, State::SwitchedOn) => true,
            (State::SwitchedOn, State::OpEnable) => true,
            (State::OpEnable, State::QuickStopActive) => true,
            (State::QuickStopActive, State::SwitchOnDisabled) => true,
            // No state change, noop
            (prev, curr) if prev == curr => false,
            (prev, curr) => unreachable!("Invalid transition: {prev:?} -> {curr:?}"),
        };

        // State transition edge trigger
        if should_transition {
            log::debug!("DS402 transition {:?} -> {:?}", self.state, next_state);

            self.state = next_state;

            self.tick_enable_op();

            Some(next_state)
        } else {
            None
        }
    }

    /// Drive state machine towards OP state
    fn tick_enable_op(&mut self) {
        match self.state {
            State::NotReadyToSwitchOn => self.set_control_word(ControlWord::STATE_SHUTDOWN),
            State::SwitchOnDisabled => self.set_control_word(ControlWord::STATE_SHUTDOWN),
            State::ReadyToSwitchOn => self.set_control_word(ControlWord::STATE_SWITCH_ON),
            State::SwitchedOn => self.set_control_word(ControlWord::STATE_ENABLE_OP),
            State::OpEnable => todo!("In op"),
            State::QuickStopActive => todo!(),
            State::FaultReactionActive => todo!(),
            State::Fault => self.set_control_word(ControlWord::STATE_FAULT_RESET),
        }
    }
}

// pub struct Ds402Sm<'a> {
//     sm: StateMachine<Ds402<'a>>,
// }

// impl<'a> Ds402Sm<'a> {
//     pub fn is_op(&self) -> bool {
//         self.sm.state == States::OpEnable
//     }
// }

// impl<'a> Ds402Sm<'a> {
//     pub fn new(context: Ds402<'a>) -> Self {
//         Self {
//             sm: StateMachine::new(context),
//         }
//     }

//     pub fn tick(&mut self) {
//         let current_state = self.sm.state.clone();

//         self.sm
//             .process_event(Events::Tick)
//             .map(|new_state| {
//                 if *new_state != current_state {
//                     log::debug!(
//                         "DS402 state transition {:?} -> {:?}",
//                         current_state,
//                         new_state
//                     );
//                 }
//             })
//             .ok();
//     }
// }

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
    const STATE_FAULT: Self = Self::from_bits_truncate(Self::FAULT.bits);

    fn mandatory(self) -> Self {
        self.intersection(Self::MANDATORY)
    }

    fn with_quickstop(self) -> Self {
        self.intersection(Self::MANDATORY | Self::QUICK_STOP)
    }

    fn as_state(&self) -> State {
        // Quick stop must equal 0 to be active
        if self.with_quickstop() == Self::STATE_OP_ENABLE {
            return State::QuickStopActive;
        }

        // Order is important here
        match self.mandatory() {
            Self::STATE_FAULT_REACTION_ACTIVE => State::FaultReactionActive,
            Self::STATE_FAULT => State::Fault,
            Self::STATE_OP_ENABLE => State::OpEnable,
            Self::STATE_READY_TO_SWITCH_ON => State::ReadyToSwitchOn,
            Self::STATE_SWITCHED_ON => State::SwitchedOn,
            Self::STATE_SWITCH_ON_DISABLED => State::SwitchOnDisabled,
            Self::STATE_NOT_READY_TO_SWITCH_ON => State::NotReadyToSwitchOn,
            s => unreachable!("Unrecognised state {:?}", s),
        }
    }
}

#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub enum State {
    NotReadyToSwitchOn,
    SwitchOnDisabled,
    ReadyToSwitchOn,
    SwitchedOn,
    OpEnable,
    QuickStopActive,
    FaultReactionActive,
    Fault,
}

#[cfg(test)]
mod tests {
    use super::*;

    // #[test]
    // fn check() {
    //     env_logger::try_init().ok();

    //     let inputs = [0x00, 0x00];
    //     let mut outputs = [0x00, 0x00];

    //     let mut sm = Ds402::new(&inputs, &mut outputs);

    //     // while !sm.is_op() {
    //     //     sm.tick();
    //     // }
    // }

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
