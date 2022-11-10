use crate::{
    error::Error,
    pdu_data::{PduData, PduRead},
    slave::SlaveRef,
    timer_factory::TimerFactory,
    SubIndex,
};
use core::fmt::Debug;

pub struct GroupSlave<'a, TIMEOUT> {
    pub(crate) slave: SlaveRef<'a, TIMEOUT>,
    pub inputs: Option<&'a [u8]>,
    pub outputs: Option<&'a mut [u8]>,
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
}
