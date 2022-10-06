use crate::{error::Error, pdi::PdiOffset, slave::Slave, timer_factory::TimerFactory, Client};

// TODO: Can probably dedupe with pdi::Pdi?
#[derive(Debug, Default)]
pub struct SlaveGroup<const MAX_SLAVES: usize> {
    slaves: heapless::Vec<Slave, MAX_SLAVES>,
}

impl<const MAX_SLAVES: usize> SlaveGroup<MAX_SLAVES> {
    pub fn push(&mut self, slave: Slave) -> Result<(), Error> {
        self.slaves.push(slave).map_err(|_| Error::TooManySlaves)
    }

    pub fn slaves(&self) -> &[Slave] {
        &self.slaves
    }

    // TODO: AsRef or AsMut trait?
    pub fn as_mut_ref(&mut self) -> SlaveGroupRef<'_> {
        SlaveGroupRef {
            slaves: self.slaves.as_mut(),
        }
    }
}

/// A reference to a [`SlaveGroup`] with elided `MAX_SLAVES` constant.
#[derive(Debug)]
pub struct SlaveGroupRef<'a> {
    // TODO: Un-pub?
    pub(crate) slaves: &'a mut [Slave],
}

impl<'a> SlaveGroupRef<'a> {
    pub(crate) async fn configure_from_eeprom<
        const MAX_FRAMES: usize,
        const MAX_PDU_DATA: usize,
        TIMEOUT,
    >(
        &mut self,
        mut offset: PdiOffset,
        client: &Client<MAX_FRAMES, MAX_PDU_DATA, TIMEOUT>,
    ) -> Result<PdiOffset, Error>
    where
        TIMEOUT: TimerFactory,
    {
        for slave in self.slaves.iter_mut() {
            offset = slave
                .configure_from_eeprom(&client, offset, &mut || async {
                    // TODO: Store PO2SO hook on slave. Currently blocked by `Client` having so many const generics
                    Ok(())
                })
                .await?;
        }

        Ok(offset)
    }
}
