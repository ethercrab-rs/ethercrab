use crate::{
    al_control::AlControl, al_status::AlState, al_status_code::AlStatusCode, client::Client,
    eeprom::Eeprom, error::Error, pdu::CheckWorkingCounter, register::RegisterAddress,
    timer_factory::TimerFactory,
};
use core::{cell::RefMut, time::Duration};
use packed_struct::PackedStruct;

#[derive(Clone, Debug)]
pub struct Slave {
    pub configured_address: u16,
    pub state: AlState,
}

impl Slave {
    pub fn new(configured_address: u16, state: AlState) -> Self {
        Self {
            configured_address,
            state,
        }
    }
}

pub struct SlaveRef<
    'a,
    const MAX_FRAMES: usize,
    const MAX_PDU_DATA: usize,
    const MAX_SLAVES: usize,
    TIMEOUT,
> {
    pub(crate) client: &'a Client<MAX_FRAMES, MAX_PDU_DATA, MAX_SLAVES, TIMEOUT>,
    pub(crate) slave: RefMut<'a, Slave>,
}

impl<'a, const MAX_FRAMES: usize, const MAX_PDU_DATA: usize, const MAX_SLAVES: usize, TIMEOUT>
    SlaveRef<'a, MAX_FRAMES, MAX_PDU_DATA, MAX_SLAVES, TIMEOUT>
where
    TIMEOUT: TimerFactory,
{
    pub fn new(
        client: &'a Client<MAX_FRAMES, MAX_PDU_DATA, MAX_SLAVES, TIMEOUT>,
        slave: RefMut<'a, Slave>,
    ) -> Self {
        Self { client, slave }
    }

    pub async fn request_slave_state(&self, state: AlState) -> Result<(), Error> {
        debug!(
            "Set state {} for slave address {:#04x}",
            state, self.slave.configured_address
        );

        // Send state request
        self.client
            .fpwr(
                self.slave.configured_address,
                RegisterAddress::AlControl,
                AlControl::new(state).pack().unwrap(),
            )
            .await?
            .wkc(1, "AL control")?;

        let res = crate::timeout::<TIMEOUT, _, _>(Duration::from_millis(1000), async {
            loop {
                let status = self
                    .client
                    .fprd::<AlControl>(self.slave.configured_address, RegisterAddress::AlStatus)
                    .await?
                    .wkc(1, "AL status")?;

                if status.state == state {
                    break Result::<(), _>::Ok(());
                }

                TIMEOUT::timer(Duration::from_millis(10)).await;
            }
        })
        .await;

        match res {
            Err(Error::Timeout) => {
                // TODO: Extract into separate method to get slave status code
                {
                    let (status, _working_counter) = self
                        .client
                        .fprd::<AlStatusCode>(
                            self.slave.configured_address,
                            RegisterAddress::AlStatusCode,
                        )
                        .await?;

                    debug!("Slave status code: {}", status);
                }

                Err(Error::Timeout)
            }
            other => other,
        }
    }

    // TODO: Separate TIMEOUT for EEPROM specifically
    pub fn eeprom(&'a self) -> Eeprom<'a, MAX_FRAMES, MAX_PDU_DATA, MAX_SLAVES, TIMEOUT> {
        Eeprom::new(self)
    }
}
