//! DS402/CiA402 high level interface.

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

#[cfg(test)]
mod tests {

    use heapless::FnvIndexMap;

    use crate::{SubDevicePdi, SubDeviceRef};

    use super::*;

    #[test]
    fn raw() {
        let outputs = &[SyncManagerAssignment {
            index: 0x1c12,
            mappings: &[PdoMapping {
                index: 0x1600,
                objects: &[WriteObject::ControlWord, WriteObject::OpMode],
            }],
        }];

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

        // PDI is locked during this closure
        ds402.update(|| {});
        // loop {
        //     group.tx_rx();
        //     ds402.tick();
        // }

        struct Ds402<'group, const MAX_PDI: usize, const ON: usize> {
            outputs: FnvIndexMap<u16, core::ops::Range<usize>, ON>,
            subdevice: SubDeviceRef<'group, SubDevicePdi<'group, MAX_PDI>>,
        }

        impl<'group, const MAX_PDI: usize, const ON: usize> Ds402<'group, MAX_PDI, ON> {
            fn set_op_mode(&mut self, mode: OpMode) -> Result<(), ()> {
                match self.outputs.get_mut(&(WriteObject::OpMode as u16)) {
                    Some(v) => {
                        v = mode;

                        Ok(())
                    }
                    None => Err(()),
                }
            }
        }

        let sd = Ds402::<32> {
            outputs: FnvIndexMap::from_iter(it),
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
