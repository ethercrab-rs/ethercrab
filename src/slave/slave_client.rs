use crate::{
    al_control::AlControl,
    al_status_code::AlStatusCode,
    client::Client,
    eeprom::{types::SiiOwner, Eeprom},
    error::Error,
    pdu_data::{PduData, PduRead},
    pdu_loop::CheckWorkingCounter,
    register::RegisterAddress,
    slave_state::SlaveState,
    timer_factory::{Timeouts, TimerFactory},
    PduLoop,
};
use core::fmt::Debug;
use packed_struct::PackedStruct;

pub struct SlaveClient<'a, const MAX_FRAMES: usize, const MAX_PDU_DATA: usize, TIMEOUT> {
    pub(in crate::slave) client: &'a Client<'a, MAX_FRAMES, MAX_PDU_DATA, TIMEOUT>,
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

    pub fn timeouts(&self) -> &Timeouts {
        &self.client.timeouts
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

    pub async fn wait_for_state(&self, desired_state: SlaveState) -> Result<(), Error> {
        crate::timer_factory::timeout::<TIMEOUT, _, _>(
            self.client.timeouts.state_transition,
            async {
                loop {
                    let status = self
                        .read::<AlControl>(RegisterAddress::AlStatus, "Read AL status")
                        .await?;

                    if status.state == desired_state {
                        break Result::<(), _>::Ok(());
                    }

                    self.client.timeouts.loop_tick::<TIMEOUT>().await;
                }
            },
        )
        .await
    }

    pub async fn request_slave_state(&self, desired_state: SlaveState) -> Result<(), Error> {
        debug!(
            "Set state {} for slave address {:#04x}",
            desired_state, self.configured_address
        );

        // Send state request
        self.write(
            RegisterAddress::AlControl,
            AlControl::new(desired_state).pack().unwrap(),
            "AL control",
        )
        .await?;

        self.wait_for_state(desired_state).await
    }

    pub async fn status(&self) -> Result<(SlaveState, AlStatusCode), Error> {
        let status = self
            .read::<AlControl>(RegisterAddress::AlStatus, "AL Status")
            .await
            .map(|ctl| ctl.state)?;

        let code = self
            .read::<AlStatusCode>(RegisterAddress::AlStatusCode, "AL Status Code")
            .await?;

        Ok((status, code))
    }

    // TODO: Separate TIMEOUT for EEPROM specifically
    pub fn eeprom(&'a self) -> Eeprom<'a, MAX_FRAMES, MAX_PDU_DATA, TIMEOUT> {
        Eeprom::new(&self)
    }

    pub async fn set_eeprom_mode(&self, mode: SiiOwner) -> Result<(), Error> {
        self.write::<u16>(RegisterAddress::SiiConfig, 2, "debug write")
            .await?;
        self.write::<u16>(RegisterAddress::SiiConfig, mode as u16, "debug write 2")
            .await?;

        Ok(())
    }
}
