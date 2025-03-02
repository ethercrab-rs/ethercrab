//! DS402/CiA402 high level interface.

use ethercrab_wire::{EtherCrabWireRead, EtherCrabWireReadWrite, EtherCrabWireSized};
use heapless::FnvIndexMap;

use crate::{fmt, SubDevicePdi, SubDeviceRef};

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

impl ControlWord {
    /// Set the desired state.
    pub fn set_state(&mut self, state: WriteState) {
        // Only reset faults if explicitly requested
        self.fault_reset = false;

        match state {
            WriteState::ResetFault => self.fault_reset = true,
            WriteState::SwitchOn => {
                self.switch_on = true;
            }
            WriteState::EnableVoltage => {
                self.switch_on = true;
                self.enable_voltage = true;
            }
            WriteState::EnableOperation => {
                self.switch_on = true;
                self.enable_voltage = true;
                self.enable_operation = true;
            }
        }
    }
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

impl StatusWord {
    /// Read various fields of the status word and return the state machine state.
    pub fn state(&self) -> ReadState {
        if self.fault {
            ReadState::Fault
        } else if self.quick_stop {
            ReadState::QuickStop
        } else if self.operation_enabled {
            ReadState::OpEnabled
        } else if self.switched_on {
            ReadState::SwitchedOn
        } else if self.ready_to_switch_on {
            ReadState::ReadyToSwitchOn
        } else if self.switch_on_disabled {
            ReadState::SwitchOnDisabled
        } else {
            ReadState::NotReadyToSwitchOn
        }
    }
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

/// Set the DS402 state machine state.
///
/// This enum is used to set certain bits in the [`ControlWord`].
#[derive(Debug, Copy, Clone)]
pub enum WriteState {
    /// Reset fault.
    ResetFault,
    /// Switch on.
    SwitchOn,
    /// Enable voltage.
    EnableVoltage,
    /// Enable operation.
    EnableOperation,
}

/// DS402 state machine state.
///
/// This enum is created from the individual bits in or [`StatusWord`].
///
/// ETG6010 5.1 Figure 2: State Machine
#[derive(Debug, Copy, Clone)]
pub enum ReadState {
    /// Not ready to switch on.
    NotReadyToSwitchOn,
    /// Switch on disabled.
    SwitchOnDisabled,
    /// Ready to switch on.
    ReadyToSwitchOn,
    /// Switched on.
    SwitchedOn,
    /// Operation enabled.
    OpEnabled,
    /// Quick stop active.
    QuickStop,
    /// The device is in a fault state.
    Fault,
}

/// State machine transition.
#[derive(Debug, Copy, Clone)]
pub enum Transition {
    /// The device is in a steady state.
    Steady(ReadState),
    /// The device is transitioning to a new desired state.
    Transitioning {
        /// Desired state.
        desired: WriteState,
        /// Current state.
        actual: ReadState,
    },
    /// The device has finished transitioning to a new state.
    Edge {
        /// Previous state before the transition started.
        previous: ReadState,
        /// Current state.
        current: ReadState,
    },
}

/// An object sent from the MainDevice to the SubDevice (RxPdo).
#[derive(Debug, Copy, Clone)]
#[repr(u32)]
pub enum WriteObject {
    /// Control word.
    ControlWord = 0x6040_0010,

    /// Operation mode.
    OpMode = 0x6060_0008,
}

/// An object received by the MainDevice from the SubDevice (TxPdo).
#[derive(Debug, Copy, Clone)]
#[repr(u32)]
pub enum ReadObject {
    /// Status word.
    StatusWord = 0x6041_0010,

    /// Operation mode.
    OpMode = 0x6061_0008,
}

/// SDO config for a SubDevice's read (with [`ReadObject`]) or write (with [`WriteObject`]) PDOs.
pub struct SyncManagerAssignment<'a, O> {
    // Sync manager, start from 0x1C10 to 0x1C2F.
    // TODO: Add an API to get SD read/write sync man by index?
    /// Sync manager, starting from `0x1c12` for sync manager 0.
    pub index: u16,

    /// PDO mappings.
    pub mappings: &'a [PdoMapping<'a, O>],
}

/// PDO object to be mapped.
pub struct PdoMapping<'a, O> {
    /// PDO index, e.g. `0x1600` or `0x1a00`.
    pub index: u16,

    /// PDO objects to map into this PDO.
    pub objects: &'a [O],
}

/// Wrap a group SubDevice in a higher level DS402 API
pub struct Ds402<'group, const MAX_PDI: usize, const MAX_OUTPUT_OBJECTS: usize> {
    outputs: FnvIndexMap<u16, core::ops::Range<usize>, MAX_OUTPUT_OBJECTS>,
    // TODO: Inputs map
    subdevice: SubDeviceRef<'group, SubDevicePdi<'group, MAX_PDI>>,
}

impl<'group, const MAX_PDI: usize, const MAX_OUTPUT_OBJECTS: usize>
    Ds402<'group, MAX_PDI, MAX_OUTPUT_OBJECTS>
{
    /// Set DS402 operation mode (CSV, CSP, etc).
    // TODO: This will be a mandatory field at some point, so this specifically doesn't need
    // to return a `Result`.
    pub fn set_op_mode(&mut self, mode: OpMode) -> Result<(), ()> {
        match self.outputs.get_mut(&(WriteObject::OpMode as u16)) {
            Some(v) => {
                // v = mode;
                todo!();

                Ok(())
            }
            None => Err(()),
        }
    }

    /// Get the DS402 status word.
    pub fn status_word(&self) -> StatusWord {
        // TODO: Dynamically(?) compute
        let state_range = 0..StatusWord::PACKED_LEN;

        fmt::unwrap_opt!(self
            .subdevice
            .inputs_raw()
            .get(state_range)
            .and_then(|bytes| StatusWord::unpack_from_slice(bytes).ok()))
    }

    /// Get the current DS402 state machine state.
    pub fn state(&self) -> ReadState {
        self.status_word().state()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use heapless::FnvIndexMap;

    #[test]
    fn raw() {
        // SM configuration. Order matters!
        // TODO: Make some fields mandatory: op mode, op mode status, supported drive modes. This is
        // required by ETG6010 Table 8: Modes of operation – Object list
        let outputs = &[SyncManagerAssignment {
            // TODO: Higher level API so we can get the correct read/write SM address from the
            // subdevice (e.g. `sd.read_sm(0) -> Option<u16>` or something)
            index: 0x1c12,
            // TODO: Validate that the SM can have this many mappings
            mappings: &[PdoMapping {
                index: 0x1600,
                // TODO: Validate that this mapping object can have this many PDOs, e.g. some SD
                // PDOs can only have 4 assignments
                objects: &[WriteObject::ControlWord, WriteObject::OpMode],
            }],
        }];

        // PDI offset accumulator
        let mut position = 0;

        let it = outputs
            .iter()
            .flat_map(|sm| sm.mappings)
            .flat_map(|mapping| mapping.objects)
            .map(|mapping| {
                let object = *mapping as u32;

                let size = (object & 0xffff) as usize;

                let range = position..(position + size);

                position += size;

                ((object >> 16) as u16, range)
            });

        let sd = Ds402::<32> {
            outputs: FnvIndexMap::from_iter(it),
            subdevice: todo!(),
        };

        for (object, pdi_range) in sd.outputs {
            println!(
                "Object {:#06x}, {} PDI bytes at {:?}",
                object,
                pdi_range.len(),
                pdi_range
            );
        }

        sd.set_op_mode(OpMode::CyclicSynchronousPosition);
    }
}
