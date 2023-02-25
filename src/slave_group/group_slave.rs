use crate::{
    error::Error,
    pdu_data::{PduData, PduRead},
    slave::{slave_client::SlaveClient, Slave, SlaveRef},
    Client, SubIndex,
};
use core::fmt::Debug;

/// A slave belonging to a given group, along with its PDI input and output data.
#[derive(Debug)]
pub struct GroupSlave<'a> {
    slave: &'a Slave,

    /// The slave's configured address.
    pub configured_address: u16,
    /// The slave's name
    pub name: &'a str,

    inputs: &'a [u8],
    // We can make these mutable later
    outputs: &'a [u8],
}

impl<'a> GroupSlave<'a> {
    pub(crate) fn new(slave: &'a Slave, inputs: &'a [u8], outputs: &'a [u8]) -> Self {
        Self {
            slave,
            inputs,
            configured_address: slave.configured_address,
            name: &slave.name,
            outputs,
        }
    }

    /// Get a tuple of (I, O) for this slave in the Process Data Image (PDI).
    pub fn io(&self) -> (&[u8], &mut [u8]) {
        (self.inputs(), self.outputs())
    }

    /// Get just the inputs for this slave in the Process Data Image (PDI).
    pub fn inputs(&self) -> &[u8] {
        self.inputs
    }

    /// Get just the outputs for this slave in the Process Data Image (PDI).
    pub fn outputs(&self) -> &mut [u8] {
        unsafe {
            core::slice::from_raw_parts_mut(self.outputs.as_ptr() as *mut u8, self.outputs.len())
        }
    }

    /// Read an SDO from this slave.
    pub async fn read_sdo<T>(
        &self,
        client: &Client<'_>,
        index: u16,
        sub_index: SubIndex,
    ) -> Result<T, Error>
    where
        T: PduData,
        <T as PduRead>::Error: Debug,
    {
        let slave = SlaveRef::new(
            SlaveClient::new(client, self.slave.configured_address),
            self.slave,
        );

        slave.read_sdo(index, sub_index).await
    }

    /// Write an SDO from this slave.
    pub async fn write_sdo<T>(
        &self,
        client: &Client<'_>,
        index: u16,
        sub_index: SubIndex,
        value: T,
    ) -> Result<(), Error>
    where
        T: PduData,
        <T as PduRead>::Error: Debug,
    {
        let slave = SlaveRef::new(
            SlaveClient::new(client, self.slave.configured_address),
            self.slave,
        );

        slave.write_sdo(index, sub_index, value).await
    }
}
