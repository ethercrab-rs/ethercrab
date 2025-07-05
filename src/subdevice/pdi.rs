use super::{IoRanges, SubDevice, SubDeviceRef};
use crate::subdevice_group::MySyncUnsafeCell;
use core::ops::{Deref, DerefMut, Range};

/// Provides a read-only reference to a slice in the PDI
pub struct PdiReadGuard<'a, const N: usize> {
    lock: spin::RwLockReadGuard<'a, MySyncUnsafeCell<[u8; N]>>,
    range: Range<usize>,
}

impl<const N: usize> Deref for PdiReadGuard<'_, N> {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        let all = unsafe { &*self.lock.get() }.as_slice();

        &all[self.range.clone()]
    }
}

/// Provides a read-write reference to a slice in the PDI
pub struct PdiWriteGuard<'a, const N: usize> {
    lock: spin::rwlock::RwLockWriteGuard<'a, MySyncUnsafeCell<[u8; N]>, crate::SpinStrategy>,
    range: Range<usize>,
}

impl<const N: usize> Deref for PdiWriteGuard<'_, N> {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        let all = unsafe { &*self.lock.get() }.as_slice(); // todo: is unsafe needed?

        &all[self.range.clone()]
    }
}

impl<const N: usize> DerefMut for PdiWriteGuard<'_, N> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.lock.get_mut()[self.range.clone()]
    }
}

/// Yields read-only references to the input and output segments of the PDI
pub struct PdiIoRawReadGuard<'a, const N: usize> {
    pdi: &'a spin::rwlock::RwLock<MySyncUnsafeCell<[u8; N]>, crate::SpinStrategy>,
    ranges: IoRanges,
}

impl<const N: usize> PdiIoRawReadGuard<'_, N> {
    pub fn inputs(&self) -> PdiReadGuard<'_, N> {
        PdiReadGuard {
            lock: self.pdi.read(),
            range: self.ranges.input.bytes.clone(),
        }
    }

    pub fn outputs(&self) -> PdiReadGuard<'_, N> {
        PdiReadGuard {
            lock: self.pdi.read(),
            range: self.ranges.output.bytes.clone(),
        }
    }
}

/// Yields read-only input and read-write output segments of the PDI
pub struct PdiIoRawWriteGuard<'a, const N: usize> {
    pdi: &'a spin::rwlock::RwLock<MySyncUnsafeCell<[u8; N]>, crate::SpinStrategy>,
    ranges: IoRanges,
}

impl<const N: usize> PdiIoRawWriteGuard<'_, N> {
    pub fn inputs(&self) -> PdiReadGuard<'_, N> {
        PdiReadGuard {
            lock: self.pdi.read(),
            range: self.ranges.input.bytes.clone(),
        }
    }

    pub fn outputs(&mut self) -> PdiWriteGuard<'_, N> {
        PdiWriteGuard {
            lock: self.pdi.write(),
            range: self.ranges.output.bytes.clone(),
        }
    }
}

/// Process Data Image (PDI) segments for a given SubDevice.
///
/// Used in conjunction with [`SubDeviceRef`].
#[doc(alias = "SlavePdi")]
pub struct SubDevicePdi<'group, const MAX_PDI: usize> {
    subdevice: &'group SubDevice,
    pdi: &'group spin::rwlock::RwLock<MySyncUnsafeCell<[u8; MAX_PDI]>, crate::SpinStrategy>,
}

unsafe impl<const MAX_PDI: usize> Send for SubDevicePdi<'_, MAX_PDI> {}
unsafe impl<const MAX_PDI: usize> Sync for SubDevicePdi<'_, MAX_PDI> {}

impl<const MAX_PDI: usize> Deref for SubDevicePdi<'_, MAX_PDI> {
    type Target = SubDevice;

    fn deref(&self) -> &Self::Target {
        self.subdevice
    }
}

impl<'group, const MAX_PDI: usize> SubDevicePdi<'group, MAX_PDI> {
    pub(crate) fn new(
        subdevice: &'group SubDevice,
        pdi: &'group spin::rwlock::RwLock<MySyncUnsafeCell<[u8; MAX_PDI]>, crate::SpinStrategy>,
    ) -> Self {
        Self { subdevice, pdi }
    }
}

/// Methods used when a SubDevice is part of a group and part of the PDI has been mapped to it.
impl<const MAX_PDI: usize> SubDeviceRef<'_, SubDevicePdi<'_, MAX_PDI>> {
    /// Get a reference to the raw inputs and outputs for this SubDevice in the Process Data Image
    /// (PDI). The inputs are read-only, while the outputs can be mutated.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use ethercrab::{
    /// #     error::Error, std::tx_rx_task, MainDevice, MainDeviceConfig, PduStorage, Timeouts,
    /// # };
    /// # async fn case() {
    /// # static PDU_STORAGE: PduStorage<8, 32> = PduStorage::new();
    /// # let (tx, rx, pdu_loop) = PDU_STORAGE.try_split().expect("can only split once");
    /// # let maindevice = MainDevice::new(pdu_loop, Timeouts::default(), MainDeviceConfig::default());
    /// let mut group = maindevice.init_single_group::<8, 8>(ethercrab::std::ethercat_now).await.expect("Init");
    /// let group = group.into_op(&maindevice).await.expect("Op");
    /// let mut subdevice = group.subdevice(&maindevice, 0).expect("No device");
    ///
    /// let mut io = subdevice.io_raw_mut();
    ///
    /// io.outputs()[0] = 0xaa;
    /// # }
    /// ```
    pub fn io_raw_mut(&self) -> PdiIoRawWriteGuard<'_, MAX_PDI> {
        PdiIoRawWriteGuard {
            pdi: self.state.pdi,
            ranges: self.state.config.io.clone(),
        }
    }

    /// Get a reference to both the inputs and outputs for this SubDevice in the Process Data Image
    /// (PDI).
    ///
    /// To get a mutable reference to the SubDevice outputs, see either
    /// [`io_raw_mut`](SubDeviceRef::io_raw_mut) or
    /// [`outputs_raw_mut`](SubDeviceRef::outputs_raw_mut).
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use ethercrab::{
    /// #     error::Error, std::tx_rx_task, MainDevice, MainDeviceConfig, PduStorage, Timeouts,
    /// # };
    /// # async fn case() {
    /// # static PDU_STORAGE: PduStorage<8, 32> = PduStorage::new();
    /// # let (tx, rx, pdu_loop) = PDU_STORAGE.try_split().expect("can only split once");
    /// # let maindevice = MainDevice::new(pdu_loop, Timeouts::default(), MainDeviceConfig::default());
    /// let mut group = maindevice.init_single_group::<8, 8>(ethercrab::std::ethercat_now).await.expect("Init");
    /// let group = group.into_op(&maindevice).await.expect("Op");
    /// let mut subdevice = group.subdevice(&maindevice, 0).expect("No device");
    ///
    /// let io = subdevice.io_raw();
    ///
    /// dbg!(io.inputs()[0]);
    ///
    /// // Not allowed to mutate the outputs
    /// // io.outputs()[0] = 0xff;
    /// // But we can read them
    /// dbg!(io.outputs()[0]);
    /// # }
    /// ```
    pub fn io_raw(&self) -> PdiIoRawReadGuard<'_, MAX_PDI> {
        PdiIoRawReadGuard {
            pdi: self.state.pdi,
            ranges: self.state.config.io.clone(),
        }
    }

    /// Get a reference to the raw input data for this SubDevice in the Process Data Image (PDI).
    pub fn inputs_raw(&self) -> PdiReadGuard<'_, MAX_PDI> {
        PdiReadGuard {
            lock: self.state.pdi.read(),
            range: self.state.config.io.input.bytes.clone(),
        }
    }

    /// Get a reference to the raw output data for this SubDevice in the Process Data Image (PDI).
    pub fn outputs_raw(&self) -> PdiReadGuard<'_, MAX_PDI> {
        PdiReadGuard {
            lock: self.state.pdi.read(),
            range: self.state.config.io.output.bytes.clone(),
        }
    }

    /// Get a mutable reference to the raw output data for this SubDevice in the Process Data Image
    /// (PDI).
    pub fn outputs_raw_mut(&self) -> PdiWriteGuard<'_, MAX_PDI> {
        PdiWriteGuard {
            lock: self.state.pdi.write(),
            range: self.state.config.io.output.bytes.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MainDevice, MainDeviceConfig, PduStorage, Timeouts, pdi::PdiSegment};

    #[test]
    fn get_inputs() {
        static PDU_STORAGE: PduStorage<8, 64> = PduStorage::new();
        let (_tx, _rx, pdu_loop) = PDU_STORAGE.try_split().expect("can only split once");
        let maindevice =
            MainDevice::new(pdu_loop, Timeouts::default(), MainDeviceConfig::default());
        let mut sd = SubDevice::default();

        sd.config.io = IoRanges {
            input: PdiSegment {
                bytes: 0..2,
                // bit_len: 16,
            },
            output: PdiSegment {
                bytes: 2..4,
                // bit_len: 16,
            },
            rx_pdos: Default::default(),
            tx_pdos: Default::default(),
        };

        const LEN: usize = 64;

        let pdi_storage = spin::rwlock::RwLock::new(MySyncUnsafeCell::new([0xabu8; LEN]));

        let pdi = SubDevicePdi::new(&sd, &pdi_storage);

        let sd_ref = SubDeviceRef::new(&maindevice, 0x1000, pdi);

        {
            let mut outputs = sd_ref.outputs_raw_mut();

            outputs[0] = 0xff;
        }

        assert_eq!(
            &pdi_storage.write().get_mut()[0..4],
            &[0xab, 0xab, 0xff, 0xab]
        );
    }
}
