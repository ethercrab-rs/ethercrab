use super::{Slave, SlaveRef};
use atomic_refcell::AtomicRefMut;
use core::ops::Deref;

/// Process Data Image (PDI) segments for a given slave device.
///
/// Used in conjunction with [`SlaveRef`].
#[derive(Debug)]
pub struct SlavePdi<'group> {
    slave: AtomicRefMut<'group, Slave>,

    inputs: &'group [u8],

    outputs: &'group mut [u8],
}

impl<'group> Deref for SlavePdi<'group> {
    type Target = Slave;

    fn deref(&self) -> &Self::Target {
        &self.slave
    }
}

impl<'group> SlavePdi<'group> {
    pub(crate) fn new(
        slave: AtomicRefMut<'group, Slave>,
        inputs: &'group [u8],
        outputs: &'group mut [u8],
    ) -> Self {
        Self {
            slave,
            inputs,
            outputs,
        }
    }
}

/// Methods used when a slave device is part of a group and part of the PDI has been mapped to it.
impl<'a, 'group> SlaveRef<'a, SlavePdi<'group>> {
    /// Get a tuple of (&I, &mut O) for this slave in the Process Data Image (PDI).
    ///
    /// # Examples
    ///
    /// ## Disallow multiple mutable references
    ///
    /// ```compile_fail,E0499
    /// // error[E0499]: cannot borrow `slave` as mutable more than once at a time
    /// # use ethercrab::{
    /// #     error::Error, std::tx_rx_task, Client, ClientConfig, PduStorage, SlaveGroup,
    /// #      SlaveGroupState, Timeouts,
    /// # };
    /// # async fn case() {
    /// # static PDU_STORAGE: PduStorage<8, 8> = PduStorage::new();
    /// # let (tx, rx, pdu_loop) = PDU_STORAGE.try_split().expect("can only split once");
    /// # let client = Client::new(pdu_loop, Timeouts::default(), ClientConfig::default());
    /// let mut group = client.init_single_group::<8, 8>(ethercrab::std::ethercat_now).await.expect("Init");
    /// let group = group.into_op(&client).await.expect("Op");
    /// let mut slave = group.slave(&client, 0).expect("No device");
    ///
    /// let (i1, o1) = slave.io_raw_mut();
    ///
    /// // Danger: second reference to mutable outputs! This will fail to copmile.
    /// let (i2, o2) = slave.io_raw_mut();
    ///
    /// o1[0] = 0xaa;
    /// # }
    /// ```
    pub fn io_raw_mut(&mut self) -> (&[u8], &mut [u8]) {
        (self.state.inputs, self.state.outputs)
    }

    /// Get a tuple of (&I, &O) for this slave in the Process Data Image (PDI).
    ///
    /// To get a mutable reference to the slave outputs, see either
    /// [`io_raw_mut`](SlaveRef::io_raw_mut) or [`outputs_raw_mut`](SlaveRef::outputs_raw_mut).
    ///
    /// # Examples
    ///
    /// ## Disallow multiple mutable references
    ///
    /// ```compile_fail,E0502
    /// // error[E0502]: cannot borrow `slave` as immutable because it is also borrowed as mutable
    /// # use ethercrab::{
    /// #     error::Error, std::tx_rx_task, Client, ClientConfig, PduStorage, SlaveGroup,
    /// #      SlaveGroupState, Timeouts,
    /// # };
    /// # async fn case() {
    /// # static PDU_STORAGE: PduStorage<8, 8> = PduStorage::new();
    /// # let (tx, rx, pdu_loop) = PDU_STORAGE.try_split().expect("can only split once");
    /// # let client = Client::new(pdu_loop, Timeouts::default(), ClientConfig::default());
    /// let mut group = client.init_single_group::<8, 8>(ethercrab::std::ethercat_now).await.expect("Init");
    /// let group = group.into_op(&client).await.expect("Op");
    /// let mut slave = group.slave(&client, 0).expect("No device");
    ///
    /// let (i1, o1_mut) = slave.io_raw_mut();
    ///
    /// // Not allowed: outputs are already mutably borrowed so we cannot hold another reference to
    /// // them until that borrow is dropped.
    /// let (i2, o2) = slave.io_raw();
    ///
    /// o1_mut[0] = 0xff;
    /// # }
    /// ```
    pub fn io_raw(&self) -> (&[u8], &[u8]) {
        (self.state.inputs, self.state.outputs)
    }

    /// Get a reference to the raw input data for this slave in the Process Data Image (PDI).
    pub fn inputs_raw(&self) -> &[u8] {
        self.state.inputs
    }

    /// Get a reference to the raw output data for this slave in the Process Data Image (PDI).
    pub fn outputs_raw(&self) -> &[u8] {
        self.state.outputs
    }

    /// Get a mutable reference to the raw output data for this slave in the Process Data Image
    /// (PDI).
    pub fn outputs_raw_mut(&mut self) -> &mut [u8] {
        self.state.outputs
    }
}
