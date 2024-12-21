use super::{IoRanges, SubDevice, SubDeviceRef};
use crate::subdevice_group::MySyncUnsafeCell;
use core::{
    cell::UnsafeCell,
    ops::{Deref, DerefMut, Range},
    ptr::NonNull,
    slice,
};

pub struct PdiReadGuard<'group> {
    guard: spin::RwLockReadGuard<'group, UnsafeCell<[u8; 0]>>,
    max_pdi: usize,
    range: Range<usize>,
}

pub struct PdiWriteGuard<'group> {
    guard: spin::RwLockWriteGuard<'group, NonNull<[u8]>>,
    // pdi: NonNull<[u8]>,
    max_pdi: usize,
    range: Range<usize>,
}

impl<'group> Deref for PdiWriteGuard<'group> {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        #[allow(trivial_casts)]
        unsafe {
            // let ptr: *const u8 = self.guard.get().cast();
            // let ptr: *const u8 = self.guard.as_ptr().cast();
            let ptr = &self.guard;

            // let ptr = ptr.byte_add(self.range.start);

            // let len = self.range.len();

            // let ptr = ptr.as_ptr();

            // // FIXME: Miri fails when length is gt 0, pretty obviously tbh
            // slice::from_raw_parts(ptr.cast() as *const u8, len)

            ptr.as_ref()
        }
    }
}

/// Process Data Image (PDI) segments for a given SubDevice.
///
/// Used in conjunction with [`SubDeviceRef`].
#[derive(Debug)]
#[doc(alias = "SlavePdi")]
pub struct SubDevicePdi<'group> {
    subdevice: &'group SubDevice,
    ranges: IoRanges,
    max_pdi: usize,
    pdi: &'group spin::RwLock<NonNull<[u8]>>,
}

unsafe impl<'group> Send for SubDevicePdi<'group> {}
unsafe impl<'group> Sync for SubDevicePdi<'group> {}

impl<'group> Deref for SubDevicePdi<'group> {
    type Target = SubDevice;

    fn deref(&self) -> &Self::Target {
        &self.subdevice
    }
}

impl<'group> SubDevicePdi<'group> {
    pub(crate) fn new<const MAX_PDI: usize>(
        subdevice: &'group SubDevice,
        max_pdi: usize,
        pdi: &spin::RwLock<MySyncUnsafeCell<[u8; MAX_PDI]>>,
        ranges: IoRanges,
    ) -> Self {
        let pdi = NonNull::from(pdi);

        let pdi: NonNull<spin::RwLock<NonNull<[u8]>>> = pdi.cast();

        let pdi = unsafe { pdi.as_ref() };

        Self {
            subdevice,
            ranges,
            max_pdi,
            pdi,
        }
    }
}

/// Methods used when a SubDevice is part of a group and part of the PDI has been mapped to it.
impl<'a, 'group> SubDeviceRef<'a, SubDevicePdi<'group>> {
    /// Get a tuple of (&I, &mut O) for this SubDevice in the Process Data Image (PDI).
    ///
    /// # Examples
    ///
    /// ## Disallow multiple mutable references
    ///
    /// ```compile_fail,E0499
    /// // error[E0499]: cannot borrow `SubDevice` as mutable more than once at a time
    /// # use ethercrab::{
    /// #     error::Error, std::tx_rx_task, MainDevice, MainDeviceConfig, PduStorage, Timeouts,
    /// # };
    /// # async fn case() {
    /// # static PDU_STORAGE: PduStorage<8, 8> = PduStorage::new();
    /// # let (tx, rx, pdu_loop) = PDU_STORAGE.try_split().expect("can only split once");
    /// # let maindevice = MainDevice::new(pdu_loop, Timeouts::default(), MainDeviceConfig::default());
    /// let mut group = maindevice.init_single_group::<8, 8>(ethercrab::std::ethercat_now).await.expect("Init");
    /// let group = group.into_op(&maindevice).await.expect("Op");
    /// let mut subdevice = group.subdevice(&maindevice, 0).expect("No device");
    ///
    /// let (i1, o1) = subdevice.io_raw_mut();
    ///
    /// // Danger: second reference to mutable outputs! This will fail to compile.
    /// let (i2, o2) = subdevice.io_raw_mut();
    ///
    /// o1[0] = 0xaa;
    /// # }
    /// ```
    pub fn io_raw_mut(&mut self) -> (&[u8], &mut [u8]) {
        // let mut lock = unsafe { self.state.pdi.as_ref() }.write();

        // let arr = lock.get_mut().as_slice();

        todo!()
    }

    /// Get a tuple of (&I, &O) for this SubDevice in the Process Data Image (PDI).
    ///
    /// To get a mutable reference to the SubDevice outputs, see either
    /// [`io_raw_mut`](SubDeviceRef::io_raw_mut) or
    /// [`outputs_raw_mut`](SubDeviceRef::outputs_raw_mut).
    ///
    /// # Examples
    ///
    /// ## Disallow multiple mutable references
    ///
    /// ```compile_fail,E0502
    /// // error[E0502]: cannot borrow `SubDevice` as immutable because it is also borrowed as mutable
    /// # use ethercrab::{
    /// #     error::Error, std::tx_rx_task, MainDevice, MainDeviceConfig, PduStorage, Timeouts,
    /// # };
    /// # async fn case() {
    /// # static PDU_STORAGE: PduStorage<8, 8> = PduStorage::new();
    /// # let (tx, rx, pdu_loop) = PDU_STORAGE.try_split().expect("can only split once");
    /// # let maindevice = MainDevice::new(pdu_loop, Timeouts::default(), MainDeviceConfig::default());
    /// let mut group = maindevice.init_single_group::<8, 8>(ethercrab::std::ethercat_now).await.expect("Init");
    /// let group = group.into_op(&maindevice).await.expect("Op");
    /// let mut subdevice = group.subdevice(&maindevice, 0).expect("No device");
    ///
    /// let (i1, o1_mut) = subdevice.io_raw_mut();
    ///
    /// // Not allowed: outputs are already mutably borrowed so we cannot hold another reference to
    /// // them until that borrow is dropped.
    /// let (i2, o2) = subdevice.io_raw();
    ///
    /// o1_mut[0] = 0xff;
    /// # }
    /// ```
    pub fn io_raw(&self) -> (&[u8], &mut [u8]) {
        // (self.state.inputs, self.state.outputs)
        todo!()
    }

    /// Get a reference to the raw input data for this SubDevice in the Process Data Image (PDI).
    pub fn inputs_raw(&self) -> &[u8] {
        // self.state.inputs
        todo!()
    }

    /// Get a reference to the raw output data for this SubDevice in the Process Data Image (PDI).
    pub fn outputs_raw(&self) -> PdiReadGuard {
        todo!()
    }

    /// Get a mutable reference to the raw output data for this SubDevice in the Process Data Image
    /// (PDI).
    pub fn outputs_raw_mut(&mut self) -> PdiWriteGuard {
        let lock = self.state.pdi.write();

        PdiWriteGuard {
            guard: lock,
            // pdi: ptr,
            max_pdi: self.state.max_pdi,
            range: self.state.ranges.output.bytes.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use core::marker::PhantomData;

    use super::*;
    use crate::{pdi::PdiSegment, MainDevice, MainDeviceConfig, PduStorage, Timeouts};

    #[test]
    fn minimal() {
        struct Owned<const N: usize> {
            data: UnsafeCell<[u8; N]>,
        }

        impl<const N: usize> Owned<N> {
            fn borrow_subslice(&self, range: Range<usize>) -> Borrowed {
                Borrowed {
                    data: unsafe { NonNull::new_unchecked(self.data.get()) },
                    range,
                    _lt: PhantomData,
                }
            }
        }

        struct Borrowed<'data> {
            data: NonNull<[u8]>,
            range: Range<usize>,
            _lt: PhantomData<&'data ()>,
        }
        impl<'data> Borrowed<'data> {
            fn as_ref(&self) -> &'data [u8] {
                let all = unsafe { self.data.as_ref() };

                &all[self.range.clone()]
            }
        }

        let owned = Owned {
            data: UnsafeCell::new([0u8; 128]),
        };

        let borrowed = owned.borrow_subslice(0..2);

        let d = borrowed.as_ref();
    }

    #[test]
    fn get_inputs() {
        static PDU_STORAGE: PduStorage<8, 64> = PduStorage::new();
        let (tx, rx, pdu_loop) = PDU_STORAGE.try_split().expect("can only split once");
        let maindevice =
            MainDevice::new(pdu_loop, Timeouts::default(), MainDeviceConfig::default());
        let sd = SubDevice::default();

        const LEN: usize = 64;

        let pdi_storage = spin::RwLock::new(MySyncUnsafeCell::new([0u8; LEN]));

        let ranges = IoRanges {
            input: PdiSegment {
                bytes: 0..2,
                bit_len: 16,
            },
            output: PdiSegment {
                bytes: 2..4,
                bit_len: 16,
            },
        };

        let pdi = SubDevicePdi::new(&sd, LEN, &pdi_storage, ranges);

        let mut sd_ref = SubDeviceRef::new(&maindevice, 0x1000, pdi);

        let outputs = sd_ref.outputs_raw_mut();

        dbg!(&*outputs);
    }
}
