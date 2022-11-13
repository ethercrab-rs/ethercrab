use crate::{
    error::Error,
    pdu_data::{PduData, PduRead},
    slave::SlaveRef,
    timer_factory::TimerFactory,
    SubIndex,
};
use core::{cell::UnsafeCell, fmt::Debug};

pub struct GroupSlave<'a, TIMEOUT> {
    pub(crate) slave: SlaveRef<'a, TIMEOUT>,
    inputs: &'a [u8],
    // We can make these mutable later
    outputs: &'a [u8],
}

impl<'a, TIMEOUT> GroupSlave<'a, TIMEOUT> {
    pub fn new(slave: SlaveRef<'a, TIMEOUT>, inputs: &'a [u8], outputs: &'a [u8]) -> Self {
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
}

impl<'a, TIMEOUT> GroupSlave<'a, TIMEOUT>
where
    TIMEOUT: TimerFactory,
{
    pub async fn read_sdo<T>(&self, index: u16, sub_index: SubIndex) -> Result<T, Error>
    where
        T: PduData,
        <T as PduRead>::Error: Debug,
    {
        self.slave.read_sdo(index, sub_index).await
    }

    pub async fn write_sdo<T>(&self, index: u16, sub_index: SubIndex, value: T) -> Result<(), Error>
    where
        T: PduData,
        <T as PduRead>::Error: Debug,
    {
        self.slave.write_sdo(index, sub_index, value).await
    }
}
