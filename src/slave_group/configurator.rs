use super::HookFn;
use crate::{
    error::Error,
    pdi::PdiOffset,
    register::RegisterAddress,
    slave::{
        configurator::{PdoDirection, SlaveConfigurator},
        slave_client::SlaveClient,
        Slave,
    },
    timer_factory::TimerFactory,
    Client,
};
use core::time::Duration;

pub struct Configurator<'a, TIMEOUT> {
    pub pdi_len: &'a mut usize,
    pub read_pdi_len: &'a mut usize,
    pub max_pdi_len: usize,
    pub start_address: &'a mut u32,
    pub group_working_counter: &'a mut u16,
    pub slaves: &'a mut [Slave],
    pub preop_safeop_hook: Option<&'a HookFn<TIMEOUT>>,
}

impl<'a, TIMEOUT> Configurator<'a, TIMEOUT>
where
    TIMEOUT: TimerFactory,
{
    pub(crate) async fn configure_from_eeprom<'client>(
        &mut self,
        mut offset: PdiOffset,
        client: &'client Client<'client, TIMEOUT>,
    ) -> Result<PdiOffset, Error>
    where
        TIMEOUT: TimerFactory,
    {
        *self.start_address = offset.start_address;

        for slave in self.slaves.iter_mut() {
            let mut slave_config = SlaveConfigurator::new(client, slave);

            // TODO: Split `configure_from_eeprom` so we can put all slaves into SAFE-OP without
            // waiting, then wait globally for all slaves to reach that state. Currently startup
            // time is extremely slow.
            // NOTE: This method requests and waits for the slave to enter PRE-OP
            slave_config.configure_mailboxes().await?;

            if let Some(hook) = self.preop_safeop_hook {
                let conf = slave_config.as_ref();

                let fut = (hook)(&conf);

                fut.await?;
            }

            // We're in PRE-OP at this point
            offset = slave_config
                .configure_fmmus(offset, PdoDirection::MasterRead)
                .await?;
        }

        *self.read_pdi_len = (offset.start_address - *self.start_address) as usize;

        log::debug!("Slave mailboxes configured and init hooks called");

        // We configured all read PDI mappings as a contiguous block in the previous loop. Now we'll
        // configure the write mappings in a separate loop. This means we have IIIIOOOO instead of
        // IOIOIO.
        for (i, slave) in self.slaves.iter_mut().enumerate() {
            let addr = slave.configured_address;
            let name = slave.name.clone();

            let mut slave_config = SlaveConfigurator::new(client, slave);

            // Still in PRE-OP
            offset = slave_config
                .configure_fmmus(offset, PdoDirection::MasterWrite)
                .await?;

            // FIXME: Just first slave or all slaves?
            // if name == "EL2004" {
            // if i == 0 {
            if false {
                log::info!("Slave {:#06x} {} DC", addr, name);
                let sl = SlaveClient::new(client, addr);

                // TODO: Pass in as config
                let cycle_time = Duration::from_millis(2).as_nanos() as u32;

                // Disable sync signals
                sl.write(RegisterAddress::DcSyncActive, 0x00u8, "disable sync")
                    .await?;

                let local_time: u32 = sl.read(RegisterAddress::DcSystemTime, "local time").await?;

                // TODO: Pass in as config
                // let startup_delay = Duration::from_millis(100).as_nanos() as u32;
                let startup_delay = 0;

                // TODO: Pass in as config
                let start_time = local_time + cycle_time + startup_delay;

                sl.write(
                    RegisterAddress::DcSyncStartTime,
                    start_time,
                    "sync start time",
                )
                .await?;

                sl.write(
                    RegisterAddress::DcSync0CycleTime,
                    cycle_time,
                    "sync cycle time",
                )
                .await?;

                // Enable cyclic operation (0th bit) and sync0 signal (1st bit)
                sl.write(RegisterAddress::DcSyncActive, 0b11u8, "enable sync0")
                    .await?;
            }

            // We're done configuring FMMUs, etc, now we can request this slave go into SAFE-OP
            slave_config.request_safe_op_nowait().await?;

            // We have both inputs and outputs at this stage, so can correctly calculate the group
            // WKC.
            *self.group_working_counter += slave.config.io.working_counter_sum();
        }

        log::debug!("Slave FMMUs configured for group. Able to move to SAFE-OP");

        let pdi_len = (offset.start_address - *self.start_address) as usize;

        if pdi_len > self.max_pdi_len {
            Err(Error::PdiTooLong {
                desired: self.max_pdi_len,
                required: pdi_len,
            })
        } else {
            *self.pdi_len = pdi_len;

            Ok(offset)
        }
    }
}
