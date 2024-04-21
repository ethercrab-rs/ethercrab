use super::ControlWord;

/// DS402 power state machine.
///
/// ETG6010 section 5.1 State Machine
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
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

impl Ds402State {
    /// Given `self` as the current state, get the next state the SM should transition to to reach
    /// the given `DesiredState`.
    pub fn next_state(&self, desired: &DesiredState) -> Self {
        match desired {
            DesiredState::Shutdown => match self {
                Self::SwitchOnDisabled => Self::NotReadyToSwitchOn,
                Self::ReadyToSwitchOn => Self::SwitchOnDisabled,
                Self::SwitchedOn => Self::ReadyToSwitchOn,
                Self::OpEnabled => Self::SwitchedOn,
                Self::QuickStop => Self::SwitchOnDisabled,

                Self::FaultReact => Self::Fault,

                Self::NotReadyToSwitchOn => Self::NotReadyToSwitchOn,
                Self::Fault => Self::Fault,
            },
            DesiredState::Op => match self {
                Self::NotReadyToSwitchOn => Self::SwitchOnDisabled,
                Self::SwitchOnDisabled => Self::ReadyToSwitchOn,
                Self::ReadyToSwitchOn => Self::SwitchedOn,
                Self::SwitchedOn => Self::OpEnabled,
                Self::QuickStop => Self::SwitchOnDisabled,

                Self::FaultReact => Self::Fault,

                Self::OpEnabled => Self::OpEnabled,
                Self::Fault => Self::Fault,
            },
        }
    }
}

/// Drive power state.
#[derive(Debug, Default, Copy, Clone, PartialEq, Eq)]
pub enum DesiredState {
    #[default]
    Shutdown,
    Op,
}
