use super::HookFn;
use crate::{
    error::Error,
    pdi::PdiOffset,
    slave::{
        configurator::{PdoDirection, SlaveConfigurator},
        Slave,
    },
    timer_factory::TimerFactory,
    Client,
};

/// A reference to a [`SlaveGroup`] with erased `MAX_SLAVES` constant.
pub struct SlaveGroupRef<'a, const MAX_FRAMES: usize, const MAX_PDU_DATA: usize, TIMEOUT> {
    pub pdi_len: &'a mut usize,
    pub read_pdi_len: &'a mut usize,
    pub max_pdi_len: usize,
    pub start_address: &'a mut u32,
    pub group_working_counter: &'a mut u16,
    pub slaves: &'a mut [Slave],
    pub preop_safeop_hook: Option<&'a HookFn<TIMEOUT, MAX_FRAMES, MAX_PDU_DATA>>,
}

impl<'a, const MAX_FRAMES: usize, const MAX_PDU_DATA: usize, TIMEOUT>
    SlaveGroupRef<'a, MAX_FRAMES, MAX_PDU_DATA, TIMEOUT>
where
    TIMEOUT: TimerFactory,
{
    pub(crate) async fn configure_from_eeprom<'client>(
        &mut self,
        mut offset: PdiOffset,
        client: &'client Client<'client, MAX_FRAMES, MAX_PDU_DATA, TIMEOUT>,
    ) -> Result<PdiOffset, Error>
    where
        TIMEOUT: TimerFactory,
    {
        *self.start_address = offset.start_address;

        for slave in self.slaves.iter_mut() {
            let mut slave_config = SlaveConfigurator::new(client, slave);

            slave_config.configure_mailboxes().await?;

            log::debug!("Slave group configured SAFE-OP");

            if let Some(hook) = self.preop_safeop_hook {
                let conf = slave_config.as_ref();

                let fut = (hook)(&conf);

                fut.await?;
            }

            offset = slave_config
                .configure_fmmus(offset, PdoDirection::MasterRead)
                .await?;

            *self.group_working_counter += slave.config.io.working_counter_sum();
        }

        *self.read_pdi_len = (offset.start_address - *self.start_address) as usize;

        log::debug!("Slave mailboxes configured and init hooks called, moving to PRE-OP");

        // We configured all read PDI mappings as a contiguous block in the previous loop. Now we'll
        // configure the write mappings in a separate loop. This means we have IIIIOOOO instead of
        // IOIOIO.
        for slave in self.slaves.iter_mut() {
            let mut slave_config = SlaveConfigurator::new(client, slave);

            offset = slave_config
                .configure_fmmus(offset, PdoDirection::MasterWrite)
                .await?;

            // We're done configuring FMMUs, etc, now we can request this slave go into SAFE-OP
            slave_config.request_safe_op().await?;
        }

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
