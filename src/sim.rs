//! In-memory process image for driving device code without a bus.
//!
//! Hands out the same `SubDeviceRef<SubDevicePdi>` a `SubDeviceGroup` would, backed by a
//! plain byte array, so application code can be exercised against a simulated device.

use crate::{
    MainDevice, SubDevice, SubDevicePdi, SubDeviceRef, subdevice_group::MySyncUnsafeCell,
};
use core::ops::Range;
use lock_api::{RawRwLock, RwLock};

/// A process image without a bus behind it.
pub struct SimPdi<const MAX_PDI: usize, R: RawRwLock = crate::DefaultLock> {
    subdevices: Vec<SubDevice>,
    pdi: RwLock<R, MySyncUnsafeCell<[u8; MAX_PDI]>>,
}

impl<const MAX_PDI: usize, R: RawRwLock> SimPdi<MAX_PDI, R> {
    /// One `(input_len, output_len)` per subdevice, laid out back to back in the image.
    pub fn new(layout: &[(usize, usize)]) -> Self {
        let mut offset = 0;
        let subdevices = layout
            .iter()
            .enumerate()
            .map(|(index, &(inputs, outputs))| {
                let mut sd = SubDevice::default();
                sd.configured_address = 0x1000 + index as u16;
                sd.index = index as u16;
                sd.config.io.input.bytes = offset..offset + inputs;
                offset += inputs;
                sd.config.io.output.bytes = offset..offset + outputs;
                offset += outputs;
                sd
            })
            .collect();
        assert!(offset <= MAX_PDI, "layout exceeds MAX_PDI");

        Self {
            subdevices,
            pdi: RwLock::new(MySyncUnsafeCell::new([0; MAX_PDI])),
        }
    }

    /// Borrow a subdevice the way `SubDeviceGroup::subdevice` would.
    pub fn subdevice<'a>(
        &'a self,
        maindevice: &'a MainDevice<'a>,
        index: usize,
    ) -> SubDeviceRef<'a, SubDevicePdi<'a, MAX_PDI, R>> {
        let sd = &self.subdevices[index];
        SubDeviceRef::new(
            maindevice,
            sd.configured_address,
            SubDevicePdi::new(sd, &self.pdi),
        )
    }

    fn ranges(&self, index: usize) -> (Range<usize>, Range<usize>) {
        let io = &self.subdevices[index].config.io;
        (io.input.bytes.clone(), io.output.bytes.clone())
    }

    /// Run `f(inputs, outputs)` on the raw image of one subdevice: the device side of the bus.
    pub fn with_io<T>(&self, index: usize, f: impl FnOnce(&mut [u8], &mut [u8]) -> T) -> T {
        let (inputs, outputs) = self.ranges(index);
        let mut guard = self.pdi.write();
        let all = guard.get_mut();
        let (i, o) = if inputs.start <= outputs.start {
            let (head, tail) = all.split_at_mut(outputs.start);
            (&mut head[inputs], &mut tail[..outputs.len()])
        } else {
            let (head, tail) = all.split_at_mut(inputs.start);
            (&mut tail[..inputs.len()], &mut head[outputs])
        };
        f(i, o)
    }
}
