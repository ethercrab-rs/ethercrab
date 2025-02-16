use ethercrab_wire::{EtherCrabWireRead, EtherCrabWireReadWrite};

/// DS402 control word (object 0x6040).
///
/// ETG6010 5.2.
#[derive(Debug, Copy, Clone, EtherCrabWireRead)]
#[wire(bytes = 2)]
pub struct ControlWord {
    #[wire(bits = 1)]
    switch_on: bool,
    #[wire(bits = 1)]
    enable_voltage: bool,
    #[wire(bits = 1)]
    quick_stop: bool,
    #[wire(bits = 1)]
    enable_operation: bool,
    #[wire(bits = 1)]
    op_specific_1: bool,
    #[wire(bits = 1)]
    op_specific_2: bool,
    #[wire(bits = 1)]
    op_specific_3: bool,
    #[wire(bits = 1)]
    fault_reset: bool,
    #[wire(bits = 1)]
    halt: bool,
    #[wire(bits = 1)]
    op_specific_4: bool,
    #[wire(bits = 1)]
    reserved: bool,
    #[wire(bits = 1)]
    manf_1: bool,
    #[wire(bits = 1)]
    manf_2: bool,
    #[wire(bits = 1)]
    manf_3: bool,
    #[wire(bits = 1)]
    manf_4: bool,
    #[wire(bits = 1)]
    manf_5: bool,
}

/// DS402 status word (object 0x6041).
///
/// ETG6010 5.3.
#[derive(Debug, Copy, Clone, EtherCrabWireRead)]
#[wire(bytes = 2)]
pub struct StatusWord {
    #[wire(bits = 1)]
    ready_to_switch_on: bool,
    #[wire(bits = 1)]
    switched_on: bool,
    #[wire(bits = 1)]
    operation_enabled: bool,
    #[wire(bits = 1)]
    fault: bool,
    #[wire(bits = 1)]
    voltage_enabled: bool,
    #[wire(bits = 1)]
    quick_stop: bool,
    #[wire(bits = 1)]
    switch_on_disabled: bool,
    #[wire(bits = 1)]
    warning: bool,
    #[wire(bits = 1)]
    manf_1: bool,
    #[wire(bits = 1)]
    remote: bool,
    #[wire(bits = 1)]
    op_specific_1: bool,
    #[wire(bits = 1)]
    internal_limit_active: bool,
    #[wire(bits = 1)]
    op_specific_2: bool,
    #[wire(bits = 1)]
    op_specific_3: bool,
    #[wire(bits = 1)]
    manf_2: bool,
    #[wire(bits = 1)]
    manf_3: bool,
}

/// Operation mode (objects 0x6060, 0x6061, 0x6502).
#[derive(Debug, Copy, Clone, EtherCrabWireReadWrite)]
#[wire(bytes = 1)]
#[repr(i8)]
pub enum OpMode {
    /// Profile position mode, "PP".
    ProfilePosition = 1,
    /// Velocity mode (frequency converter), "VL".
    Velocity = 2,
    /// Profile velocity mode, "PV".
    ProfileVelocity = 3,
    /// Torque profile mode, "TQ".
    ProfileTorque = 4,
    /// Homing mode, "HM".
    Homing = 6,
    /// Interpolated position mode, "IP".
    InterpolatedPosition = 7,
    /// Cyclic synchronous position mode, "CSP".
    CyclicSynchronousPosition = 8,
    /// Cyclic synchronous velocity mode, "CSV".
    CyclicSynchronousVelocity = 9,
    /// Cyclic synchronous torque mode, "CST".
    CyclicSynchronousTorque = 10,
    /// Cyclic synchronous torque mode with commutation angle, "CSTCA".
    CyclicSynchronousTorqueWithCommutation = 11,
    /// Manufacturer specific mode from `-128..=-1`.
    #[wire(catch_all)]
    ManufacturerSpecific(i8),
}
