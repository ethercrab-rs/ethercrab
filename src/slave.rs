use crate::{
    al_control::AlControl,
    al_status::AlState,
    al_status_code::AlStatusCode,
    client::Client,
    error::Error,
    pdu::CheckWorkingCounter,
    register::RegisterAddress,
    sii::{SiiControl, SiiRequest},
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
    client: &'a Client<MAX_FRAMES, MAX_PDU_DATA, MAX_SLAVES, TIMEOUT>,
    slave: RefMut<'a, Slave>,
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

    pub async fn read_eeprom_raw(&self, eeprom_address: impl Into<u16>) -> Result<u32, Error> {
        let eeprom_address: u16 = eeprom_address.into();

        // TODO: Check EEPROM error flags

        let setup = SiiRequest::read(eeprom_address);

        // Set up an SII read. This writes the control word and the register word after it
        self.client
            .fpwr(
                self.slave.configured_address,
                RegisterAddress::SiiControl,
                setup.to_array(),
            )
            .await?
            .wkc(1, "SII read setup")?;

        // TODO: Configurable timeout
        let timeout = core::time::Duration::from_millis(10);

        crate::timeout::<TIMEOUT, _, _>(timeout, async {
            loop {
                trace!("Busy loop");

                let control = self
                    .client
                    .fprd::<SiiControl>(self.slave.configured_address, RegisterAddress::SiiControl)
                    .await?
                    .wkc(1, "SII busy wait")?;

                if control.busy == false {
                    break Result::<(), Error>::Ok(());
                }

                // TODO: Configurable loop tick
                TIMEOUT::timer(core::time::Duration::from_millis(1)).await;
            }
        })
        .await?;

        let data = self
            .client
            .fprd::<u32>(self.slave.configured_address, RegisterAddress::SiiData)
            .await?
            .wkc(1, "SII data")?;

        Ok(data)
    }
}
