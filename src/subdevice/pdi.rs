use super::{IoRanges, SubDevice, SubDeviceRef};
use crate::subdevice_group::MySyncUnsafeCell;
use core::{
    marker::PhantomData,
    ops::{Deref, DerefMut, Range},
};
use lock_api::{RawRwLock, RwLock, RwLockReadGuard, RwLockWriteGuard};

pub struct PdiReadGuard<'a, const N: usize, R: RawRwLock> {
    lock: RwLockReadGuard<'a, R, MySyncUnsafeCell<[u8; N]>>,
    range: Range<usize>,
    _lt: PhantomData<&'a ()>,
}

impl<const N: usize, R: RawRwLock> Deref for PdiReadGuard<'_, N, R> {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        let all = unsafe { &*self.lock.get() }.as_slice();

        &all[self.range.clone()]
    }
}

pub struct PdiIoRawReadGuard<'a, const N: usize, R: RawRwLock> {
    lock: RwLockReadGuard<'a, R, MySyncUnsafeCell<[u8; N]>>,
    ranges: IoRanges,
    _lt: PhantomData<&'a ()>,
}

impl<const N: usize, R: RawRwLock> PdiIoRawReadGuard<'_, N, R> {
    pub fn inputs(&self) -> &[u8] {
        let all = unsafe { &*self.lock.get() }.as_slice();

        &all[self.ranges.input.bytes.clone()]
    }

    pub fn outputs(&self) -> &[u8] {
        let all = unsafe { &*self.lock.get() }.as_slice();

        &all[self.ranges.output.bytes.clone()]
    }
}

pub struct PdiIoRawWriteGuard<'a, const N: usize, R: RawRwLock> {
    lock: RwLockWriteGuard<'a, R, MySyncUnsafeCell<[u8; N]>>,
    ranges: IoRanges,
    _lt: PhantomData<&'a ()>,
}

impl<const N: usize, R: RawRwLock> PdiIoRawWriteGuard<'_, N, R> {
    pub fn inputs(&self) -> &[u8] {
        let all = unsafe { &*self.lock.get() }.as_slice();

        &all[self.ranges.input.bytes.clone()]
    }

    pub fn outputs(&mut self) -> &mut [u8] {
        let all = unsafe { &mut *self.lock.get() }.as_mut_slice();

        &mut all[self.ranges.output.bytes.clone()]
    }
}

pub struct PdiWriteGuard<'a, const N: usize, R: RawRwLock> {
    lock: RwLockWriteGuard<'a, R, MySyncUnsafeCell<[u8; N]>>,
    range: Range<usize>,
    _lt: PhantomData<&'a ()>,
}

impl<const N: usize, R: RawRwLock> Deref for PdiWriteGuard<'_, N, R> {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        let all = unsafe { &*self.lock.get() }.as_slice();

        &all[self.range.clone()]
    }
}

impl<const N: usize, R: RawRwLock> DerefMut for PdiWriteGuard<'_, N, R> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.lock.get_mut()[self.range.clone()]
    }
}

/// Process Data Image (PDI) segments for a given SubDevice.
///
/// Used in conjunction with [`SubDeviceRef`].
#[doc(alias = "SlavePdi")]
pub struct SubDevicePdi<'group, const MAX_PDI: usize, R: RawRwLock> {
    subdevice: &'group SubDevice,
    pdi: &'group RwLock<R, MySyncUnsafeCell<[u8; MAX_PDI]>>,
}

unsafe impl<const MAX_PDI: usize, R: RawRwLock> Send for SubDevicePdi<'_, MAX_PDI, R> {}
unsafe impl<const MAX_PDI: usize, R: RawRwLock> Sync for SubDevicePdi<'_, MAX_PDI, R> {}

impl<const MAX_PDI: usize, R: RawRwLock> Deref for SubDevicePdi<'_, MAX_PDI, R> {
    type Target = SubDevice;

    fn deref(&self) -> &Self::Target {
        self.subdevice
    }
}

impl<'group, const MAX_PDI: usize, R: RawRwLock> SubDevicePdi<'group, MAX_PDI, R> {
    pub(crate) fn new(
        subdevice: &'group SubDevice,
        pdi: &'group RwLock<R, MySyncUnsafeCell<[u8; MAX_PDI]>>,
    ) -> Self {
        Self { subdevice, pdi }
    }
}

/// Methods used when a SubDevice is part of a group and part of the PDI has been mapped to it.
impl<const MAX_PDI: usize, R: RawRwLock> SubDeviceRef<'_, SubDevicePdi<'_, MAX_PDI, R>> {
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
    pub fn io_raw_mut(&self) -> PdiIoRawWriteGuard<'_, MAX_PDI, R> {
        PdiIoRawWriteGuard {
            lock: self.state.pdi.write(),
            ranges: self.state.config.io.clone(),
            _lt: PhantomData,
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
    pub fn io_raw(&self) -> PdiIoRawReadGuard<'_, MAX_PDI, R> {
        PdiIoRawReadGuard {
            lock: self.state.pdi.read(),
            ranges: self.state.config.io.clone(),
            _lt: PhantomData,
        }
    }

    /// Get a reference to the raw input data for this SubDevice in the Process Data Image (PDI).
    pub fn inputs_raw(&self) -> PdiReadGuard<'_, MAX_PDI, R> {
        PdiReadGuard {
            lock: self.state.pdi.read(),
            range: self.state.config.io.input.bytes.clone(),
            _lt: PhantomData,
        }
    }

    /// Get a reference to the raw output data for this SubDevice in the Process Data Image (PDI).
    pub fn outputs_raw(&self) -> PdiReadGuard<'_, MAX_PDI, R> {
        PdiReadGuard {
            lock: self.state.pdi.read(),
            range: self.state.config.io.output.bytes.clone(),
            _lt: PhantomData,
        }
    }

    /// Get a mutable reference to the raw output data for this SubDevice in the Process Data Image
    /// (PDI).
    pub fn outputs_raw_mut(&self) -> PdiWriteGuard<'_, MAX_PDI, R> {
        PdiWriteGuard {
            lock: self.state.pdi.write(),
            range: self.state.config.io.output.bytes.clone(),
            _lt: PhantomData,
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
        };

        const LEN: usize = 64;

        let pdi_storage =
            RwLock::<crate::DefaultLock, _>::new(MySyncUnsafeCell::new([0xabu8; LEN]));

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
