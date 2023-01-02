use crate::{
    al_control::AlControl,
    al_status_code::AlStatusCode,
    client::Client,
    eeprom::{types::SiiOwner, Eeprom},
    error::Error,
    pdu_data::{PduData, PduRead},
    pdu_loop::{CheckWorkingCounter, PduResponse},
    register::RegisterAddress,
    slave_state::SlaveState,
    timer_factory::{Timeouts, TimerFactory},
};
use core::fmt::Debug;
use packed_struct::PackedStruct;

#[derive(Debug)]
pub struct SlaveClient<'a, TIMEOUT> {
    pub(in crate::slave) client: &'a Client<'a, TIMEOUT>,
    configured_address: u16,
}

impl<'a, TIMEOUT> SlaveClient<'a, TIMEOUT>
where
    TIMEOUT: TimerFactory,
{
    pub fn new(client: &'a Client<'a, TIMEOUT>, configured_address: u16) -> Self {
        Self {
            client,
            configured_address,
        }
    }

    pub fn mailbox_counter(&self) -> u8 {
        self.client.mailbox_counter()
    }

    pub fn timeouts(&self) -> &Timeouts {
        &self.client.timeouts
    }

    pub(crate) async fn read_ignore_wkc<T>(
        &self,
        register: RegisterAddress,
    ) -> Result<PduResponse<T>, Error>
    where
        T: PduRead,
        <T as PduRead>::Error: Debug,
    {
        self.client.fprd(self.configured_address, register).await
    }

    /// A wrapper around an FPWR service to this slave's configured address.
    pub(crate) async fn write_ignore_wkc<T>(
        &self,
        register: impl Into<u16>,
        value: T,
    ) -> Result<PduResponse<T>, Error>
    where
        T: PduData,
        <T as PduRead>::Error: Debug,
    {
        self.client
            .fpwr(self.configured_address, register, value)
            .await
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
        self.read_ignore_wkc(register).await?.wkc(1, context)
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
        self.write_ignore_wkc(register, value)
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
                        break Ok(());
                    }

                    self.client.timeouts.loop_tick::<TIMEOUT>().await;
                }
            },
        )
        .await
    }

    pub async fn request_slave_state_nowait(&self, desired_state: SlaveState) -> Result<(), Error> {
        debug!(
            "Set state {} for slave address {:#04x}",
            desired_state, self.configured_address
        );

        // Send state request
        let response = self
            .write(
                RegisterAddress::AlControl,
                AlControl::new(desired_state).pack().unwrap(),
                "AL control",
            )
            .await
            .and_then(|raw: [u8; 2]| AlControl::unpack(&raw).map_err(|_| Error::StateTransition))?;

        if response.error {
            let error: AlStatusCode = self.read(RegisterAddress::AlStatus, "AL status").await?;

            log::error!(
                "Error occurred transitioning slave {:#06x} to {:?}: {}",
                self.configured_address,
                desired_state,
                error,
            );

            return Err(Error::StateTransition);
        }

        Ok(())
    }

    pub async fn request_slave_state(&self, desired_state: SlaveState) -> Result<(), Error> {
        self.request_slave_state_nowait(desired_state).await?;

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

    pub fn eeprom(&'a self) -> Eeprom<'a, TIMEOUT> {
        Eeprom::new(self)
    }

    pub async fn set_eeprom_mode(&self, mode: SiiOwner) -> Result<(), Error> {
        self.write::<u16>(RegisterAddress::SiiConfig, 2, "debug write")
            .await?;
        self.write::<u16>(RegisterAddress::SiiConfig, mode as u16, "debug write 2")
            .await?;

        Ok(())
    }
}
