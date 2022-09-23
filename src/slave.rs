use crate::{
    al_control::AlControl,
    al_status::AlState,
    al_status_code::AlStatusCode,
    client::Client,
    eeprom::{
        types::{Pdo, SyncManagerEnable},
        Eeprom,
    },
    error::Error,
    pdu::CheckWorkingCounter,
    register::RegisterAddress,
    sync_manager_channel::{self, SyncManagerChannel},
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
    // DELETEME
    pub configured_address: u16,
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
        let configured_address = slave.configured_address;

        Self {
            client,
            slave,
            configured_address,
        }
    }

    pub async fn request_slave_state(&self, state: AlState) -> Result<(), Error> {
        debug!(
            "Set state {} for slave address {:#04x}",
            state, self.slave.configured_address
        );

        let addr = self.slave.configured_address;

        // Send state request
        self.client
            .fpwr(
                addr,
                RegisterAddress::AlControl,
                AlControl::new(state).pack().unwrap(),
            )
            .await?
            .wkc(1, "AL control")?;

        let res = crate::timeout::<TIMEOUT, _, _>(Duration::from_millis(1000), async {
            loop {
                let status = self
                    .client
                    .fprd::<AlControl>(addr, RegisterAddress::AlStatus)
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
                        .fprd::<AlStatusCode>(addr, RegisterAddress::AlStatusCode)
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
        Eeprom::new(self.slave.configured_address, self.client)
    }

    async fn sync_manager_config(
        &self,
        index: usize,
        rx_pdos: &[Pdo],
    ) -> Result<Option<SyncManagerChannel>, Error> {
        let sync_managers = self.eeprom().sync_managers().await?;

        if let Some(write_sm) = sync_managers.get(index) {
            let bit_len = rx_pdos
                .iter()
                .filter(|pdo| usize::from(pdo.sync_manager) == index)
                .flat_map(|pdo| {
                    pdo.entries
                        .iter()
                        .map(|entry| u16::from(entry.data_length_bits))
                })
                .sum::<u16>();

            log::debug!("Sync manager {index} has bit length {bit_len}");

            // TODO: What happens if bit_len is zero?

            let config = SyncManagerChannel {
                physical_start_address: write_sm.start_addr,
                length: u16::from(bit_len),
                control: write_sm.control,
                status: Default::default(),
                enable: sync_manager_channel::Enable {
                    enable: write_sm.enable.contains(SyncManagerEnable::ENABLE),
                    ..Default::default()
                },
            };

            Ok(Some(config))
        } else {
            Ok(None)
        }
    }

    // TODO: Because bit/byte offsets are cumulative, the slave config needs to be controlled by
    // `Client`, or at least have the base offsets fed into it.
    pub async fn configure_from_eeprom(&self) -> Result<(), Error> {
        // TODO: Check if mailbox is supported or not; autoconfig is different if it is.

        let rx_pdos = self.eeprom().rxpdos().await?;

        dbg!(&rx_pdos);

        if let Some(tx_config) = self.sync_manager_config(0, &rx_pdos).await? {
            self.client
                .fpwr(
                    self.slave.configured_address,
                    RegisterAddress::Sm0,
                    tx_config.pack().unwrap(),
                )
                .await?
                .wkc(1, "SM0")?;
        }

        Ok(())
    }
}
