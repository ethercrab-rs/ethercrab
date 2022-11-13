use crate::{
    error::Error,
    pdu_data::{PduData, PduRead},
    slave::{slave_client::SlaveClient, Slave, SlaveRef},
    timer_factory::TimerFactory,
    Client, SubIndex,
};
use core::{cell::UnsafeCell, fmt::Debug};

pub struct GroupSlave<'a> {
    slave: &'a Slave,
    inputs: &'a [u8],
    // We can make these mutable later
    outputs: &'a [u8],
}

impl<'a> GroupSlave<'a> {
    pub fn new(slave: &'a Slave, inputs: &'a [u8], outputs: &'a [u8]) -> Self {
        Self {
            slave,
            inputs,
            outputs,
        }
    }

    pub fn io(&self) -> (&[u8], &mut [u8]) {
        (self.inputs(), self.outputs())
    }

    pub fn inputs(&self) -> &[u8] {
        self.inputs
    }

    pub fn outputs(&self) -> &mut [u8] {
        unsafe {
            core::slice::from_raw_parts_mut(self.outputs.as_ptr() as *mut u8, self.outputs.len())
        }
    }

    pub async fn read_sdo<T>(
        &self,
        client: &Client<'_, impl TimerFactory>,
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

    pub async fn write_sdo<T>(
        &self,
        client: &Client<'_, impl TimerFactory>,
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
