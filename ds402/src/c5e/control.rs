use super::Ds402State;

/// ETG6010 5.2 Controlword Object
#[derive(ethercrab::EtherCrabWireWrite, Debug, Default, Copy, Clone)]
#[wire(bytes = 2)]
pub struct ControlWord {
    /// 0 Switch on, mandatory.
    #[wire(bits = 1)]
    pub switch_on: bool,
    /// 1 Enable voltage, mandatory.
    #[wire(bits = 1)]
    pub enable_voltage: bool,
    /// 2 Quick stop, optional.
    #[wire(bits = 1)]
    pub quick_stop: bool,
    /// 3 Enable operation, mandatory.
    #[wire(bits = 1)]
    pub enable_op: bool,
    /// 4 - 6 Operation mode specific, optional.
    #[wire(bits = 1)]
    pub op_specific_0: bool,
    /// 4 - 6 Operation mode specific, optional.
    #[wire(bits = 1)]
    pub op_specific_1: bool,
    /// 4 - 6 Operation mode specific, optional.
    #[wire(bits = 1)]
    pub op_specific_2: bool,
    /// 7 Fault reset, mandatory.
    #[wire(bits = 1)]
    pub fault_reset: bool,
    /// 8 Halt, optional.
    #[wire(bits = 1)]
    pub halt: bool,
    /// 9 Operation mode specific, optional.
    #[wire(bits = 1, post_skip = 1)]
    pub op_specific_3: bool,
    // /// 10 reserved, optional.
    // #[wire(bits = 1)]
    // pub _reserved: bool,
    /// 11 - 15 Manufacturer specific, optional.
    #[wire(bits = 1)]
    pub man_0: bool,
    /// 11 - 15 Manufacturer specific, optional.
    #[wire(bits = 1)]
    pub man_1: bool,
    /// 11 - 15 Manufacturer specific, optional.
    #[wire(bits = 1)]
    pub man_2: bool,
    /// 11 - 15 Manufacturer specific, optional.
    #[wire(bits = 1)]
    pub man_3: bool,
    /// 11 - 15 Manufacturer specific, optional.
    #[wire(bits = 1)]
    pub man_4: bool,
}
