use super::{Slave, SlaveRef};
use core::borrow::Borrow;

/// Process Data Image (PDI) segments for a given slave device.
#[derive(Debug)]
pub struct SlavePdi<'group> {
    slave: &'group Slave,

    inputs: &'group [u8],

    /// Made mutable when accessed.
    outputs: &'group [u8],
}

impl<'group> Borrow<Slave> for SlavePdi<'group> {
    fn borrow(&self) -> &Slave {
        self.slave
    }
}

impl<'group> SlavePdi<'group> {
    pub(crate) fn new(slave: &'group Slave, inputs: &'group [u8], outputs: &'group [u8]) -> Self {
        Self {
            slave,
            inputs,
            outputs,
        }
    }
}

/// Methods used when a slave device is part of a group and part of the PDI has been mapped to it.
impl<'a, 'group> SlaveRef<'a, SlavePdi<'group>> {
    /// Get a tuple of (I, O) for this slave in the Process Data Image (PDI).
    pub fn io_raw(&self) -> (&[u8], &mut [u8]) {
        (self.inputs_raw(), self.outputs_raw())
    }

    /// Get just the inputs for this slave in the Process Data Image (PDI).
    pub fn inputs_raw(&self) -> &[u8] {
        self.state.inputs
    }

    /// Get just the outputs for this slave in the Process Data Image (PDI).
    pub fn outputs_raw(&self) -> &mut [u8] {
        unsafe {
            core::slice::from_raw_parts_mut(
                self.state.outputs.as_ptr() as *mut u8,
                self.state.outputs.len(),
            )
        }
    }
}
