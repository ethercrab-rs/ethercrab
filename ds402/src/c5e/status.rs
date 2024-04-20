use super::Ds402State;

/// ETG6010 5.3 Statusword Object
#[derive(ethercrab::EtherCrabWireRead, Debug)]
#[wire(bytes = 2)]
pub struct StatusWord {
    /// 0 Ready to switch on, mandatory
    #[wire(bits = 1)]
    pub ready_to_switch_on: bool,
    /// 1 Switched on, mandatory
    #[wire(bits = 1)]
    pub switched_on: bool,
    /// 2 Operation enabled, mandatory
    #[wire(bits = 1)]
    pub op_enabled: bool,
    /// 3 Fault, mandatory
    #[wire(bits = 1)]
    pub fault: bool,
    /// 4 Voltage enabled, optional
    #[wire(bits = 1)]
    pub voltage_enabled: bool,
    /// 5 Quick stop, optional
    #[wire(bits = 1)]
    pub quick_stop: bool,
    /// 6 Switch on disabled, mandatory
    #[wire(bits = 1)]
    pub switch_on_disabled: bool,
    /// 7 Warning, optional
    #[wire(bits = 1)]
    pub warning: bool,
    /// 8 Manufacturer specific, optional
    #[wire(bits = 1)]
    pub man_0: bool,
    /// 9 Remote, optional
    #[wire(bits = 1)]
    pub remote: bool,
    /// 10 Operation mode specific, optional
    #[wire(bits = 1)]
    pub op_specific_0: bool,
    /// 11 Internal limit active, optional
    #[wire(bits = 1)]
    pub limit: bool,
    /// 12 Operation mode specific, conditional: mandatory for csp, csv, cst mode
    #[wire(bits = 1)]
    pub op_specific_1: bool,
    /// 13 Operation mode specific, optional
    #[wire(bits = 1)]
    pub op_specific_2: bool,
    /// 14 - 15 Manufacturer specific, optional
    #[wire(bits = 1)]
    pub man_1: bool,
    /// 14 - 15 Manufacturer specific, optional
    #[wire(bits = 1)]
    pub man_2: bool,
}

impl StatusWord {
    pub fn state(&self) -> Ds402State {
        let Self {
            switch_on_disabled,
            ready_to_switch_on,
            op_enabled,
            switched_on,
            fault,
            ..
        } = *self;

        if fault {
            return Ds402State::Fault;
        }

        if switch_on_disabled {
            if ready_to_switch_on {
                if switched_on {
                    if op_enabled {
                        Ds402State::OpEnabled
                    } else {
                        Ds402State::SwitchedOn
                    }
                } else {
                    Ds402State::ReadyToSwitchOn
                }
            } else {
                Ds402State::SwitchOnDisabled
            }
        } else {
            Ds402State::NotReadyToSwitchOn
        }
    }
}
