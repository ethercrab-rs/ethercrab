use super::{SubDevice, SubDeviceRef};
use atomic_refcell::AtomicRefMut;
use core::ops::Deref;

/// Process Data Image (PDI) segments for a given SubDevice.
///
/// Used in conjunction with [`SubDeviceRef`].
#[derive(Debug)]
#[doc(alias = "SlavePdi")]
pub struct SubDevicePdi<'group> {
    subdevice: AtomicRefMut<'group, SubDevice>,

    inputs: &'group [u8],

    outputs: &'group mut [u8],
}

impl<'group> Deref for SubDevicePdi<'group> {
    type Target = SubDevice;

    fn deref(&self) -> &Self::Target {
        &self.subdevice
    }
}

impl<'group> SubDevicePdi<'group> {
    pub(crate) fn new(
        subdevice: AtomicRefMut<'group, SubDevice>,
        inputs: &'group [u8],
        outputs: &'group mut [u8],
    ) -> Self {
        Self {
            subdevice,
            inputs,
            outputs,
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
    /// // Danger: second reference to mutable outputs! This will fail to copmile.
    /// let (i2, o2) = subdevice.io_raw_mut();
    ///
    /// o1[0] = 0xaa;
    /// # }
    /// ```
    pub fn io_raw_mut(&mut self) -> (&[u8], &mut [u8]) {
        (self.state.inputs, self.state.outputs)
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
    pub fn io_raw(&self) -> (&[u8], &[u8]) {
        (self.state.inputs, self.state.outputs)
    }

    /// Get a reference to the raw input data for this SubDevice in the Process Data Image (PDI).
    pub fn inputs_raw(&self) -> &[u8] {
        self.state.inputs
    }

    /// Get a reference to the raw output data for this SubDevice in the Process Data Image (PDI).
    pub fn outputs_raw(&self) -> &[u8] {
        self.state.outputs
    }

    /// Get a mutable reference to the raw output data for this SubDevice in the Process Data Image
    /// (PDI).
    pub fn outputs_raw_mut(&mut self) -> &mut [u8] {
        self.state.outputs
    }
}
