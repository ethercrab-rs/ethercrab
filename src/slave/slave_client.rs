use crate::{
    al_control::AlControl,
    client::Client,
    command::Command,
    error::Error,
    pdu_data::{PduData, PduRead},
    pdu_loop::{CheckWorkingCounter, PduLoopRef, PduResponse},
    register::RegisterAddress,
    slave_state::SlaveState,
    timer_factory::TimerFactory,
    PduLoop,
};
use core::{fmt::Debug, time::Duration};

pub struct SlaveClient<'a, const MAX_FRAMES: usize, const MAX_PDU_DATA: usize, TIMEOUT> {
    client: &'a Client<'a, MAX_FRAMES, MAX_PDU_DATA, TIMEOUT>,
    configured_address: u16,
}

impl<'a, const MAX_FRAMES: usize, const MAX_PDU_DATA: usize, TIMEOUT>
    SlaveClient<'a, MAX_FRAMES, MAX_PDU_DATA, TIMEOUT>
where
    TIMEOUT: TimerFactory,
{
    pub fn new(
        client: &'a Client<'a, MAX_FRAMES, MAX_PDU_DATA, TIMEOUT>,
        configured_address: u16,
    ) -> Self {
        Self {
            client,
            configured_address,
        }
    }

    pub fn mailbox_counter(&self) -> u8 {
        self.client.mailbox_counter()
    }

    // DELETEME: Leaky abstraction
    pub fn pdu_loop(&self) -> &PduLoop<MAX_FRAMES, MAX_PDU_DATA, TIMEOUT> {
        &self.client.pdu_loop
    }

    pub(crate) async fn read<T>(
        &self,
        register: RegisterAddress,
        context: &'static str,
    ) -> Result<T, Error>
    where
        T: PduRead,
        <T as PduRead>::Error: Debug,
    {
        self.client
            .fprd(self.configured_address, register)
            .await?
            .wkc(1, context)
    }

    /// A wrapper around an FPWR service to this slave's configured address.
    pub(crate) async fn write<T>(
        &self,
        register: impl Into<u16>,
        value: T,
        context: &'static str,
    ) -> Result<T, Error>
    where
        T: PduData,
        <T as PduRead>::Error: Debug,
    {
        self.client
            .fpwr(self.configured_address, register, value)
            .await?
            .wkc(1, context)
    }

    async fn wait_for_state(&self, desired_state: SlaveState) -> Result<(), Error> {
        crate::timeout::<TIMEOUT, _, _>(Duration::from_millis(5000), async {
            loop {
                let status = self
                    .read::<AlControl>(RegisterAddress::AlStatus, "Read AL status")
                    .await?;

                if status.state == desired_state {
                    break Result::<(), _>::Ok(());
                }

                TIMEOUT::timer(Duration::from_millis(10)).await;
            }
        })
        .await
    }
}
