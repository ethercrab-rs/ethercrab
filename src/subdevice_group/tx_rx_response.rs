/// Response information from transmitting the Process Data Image (PDI).
#[derive(Debug, PartialEq)]
pub struct TxRxResponse<T = ()> {
    /// Working counter.
    pub working_counter: u16,

    /// Additional data, for example a [`CycleInfo`](crate::subdevice_group::CycleInfo) struct
    /// holding Distributed Clocks information.
    pub extra: T,
}
