use crate::{
    al_control::AlControl,
    al_status::AlState,
    al_status_code::AlStatusCode,
    client::Client,
    eeprom::{
        self,
        types::{FmmuUsage, MailboxConfig, Pdo, SyncManagerEnable, SyncManagerType},
        Eeprom,
    },
    error::Error,
    fmmu::Fmmu,
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

    pub async fn configure_from_eeprom(
        &self,
        mut offset: MappingOffset,
    ) -> Result<MappingOffset, Error> {
        // TODO: Check if mailbox is supported or not; autoconfig is different if it is.

        // RX from the perspective of the slave device
        let rx_pdos = self.eeprom().rxpdos().await?;
        let tx_pdos = self.eeprom().txpdos().await?;

        let sync_managers = self.eeprom().sync_managers().await?;
        // let fmmu_usage = self.eeprom().fmmus().await?;
        let fmmu_sm_mappings = self.eeprom().fmmu_mappings().await?;

        log::trace!("rx_pdos {:#?}", rx_pdos);
        log::trace!("tx_pdos {:#?}", rx_pdos);
        log::trace!("sync_managers {:#?}", sync_managers);
        log::trace!("fmmu_sm_mappings {:#?}", fmmu_sm_mappings);

        // TODO: Actually set this up. Could we use some nice typestates to only expose mailbox
        // methods on slaves that support them? Probably not, right? Cos we need to keep lists of
        // slaves somewhere.
        let has_mailbox = sync_managers.iter().any(|sm| {
            matches!(
                sm.usage_type,
                SyncManagerType::MailboxIn | SyncManagerType::MailboxOut
            )
        });

        for (sync_manager_index, sync_manager) in sync_managers.iter().enumerate() {
            let sync_manager_index = sync_manager_index as u8;

            match sync_manager.usage_type {
                SyncManagerType::Unknown => continue,
                SyncManagerType::MailboxOut | SyncManagerType::MailboxIn => {
                    let sm_config = SyncManagerChannel {
                        physical_start_address: sync_manager.start_addr,
                        length_bytes: sync_manager.length,
                        control: sync_manager.control,
                        status: Default::default(),
                        enable: sync_manager_channel::Enable {
                            enable: sync_manager.enable.contains(SyncManagerEnable::ENABLE),
                            ..Default::default()
                        },
                    };

                    self.client
                        .fpwr(
                            self.slave.configured_address,
                            RegisterAddress::sync_manager(sync_manager_index),
                            sm_config.pack().unwrap(),
                        )
                        .await?
                        .wkc(1, "SM")?;
                }
                SyncManagerType::ProcessDataOut | SyncManagerType::ProcessDataIn => {
                    let pdos = if sync_manager.usage_type == SyncManagerType::ProcessDataOut {
                        &rx_pdos
                    } else {
                        &tx_pdos
                    };

                    let bit_len = pdos
                        .iter()
                        .filter(|pdo| pdo.sync_manager == sync_manager_index)
                        .flat_map(|pdo| {
                            pdo.entries
                                .iter()
                                .map(|entry| u16::from(entry.data_length_bits))
                        })
                        .sum::<u16>();

                    let byte_len = u16::from((bit_len + 7) / 8);

                    log::trace!(
                        "Sync manager {sync_manager_index} ({:?}) has bit length {bit_len}",
                        sync_manager.usage_type
                    );

                    let fmmu_index = fmmu_sm_mappings
                        .iter()
                        .find(|fmmu| fmmu.sync_manager == sync_manager_index)
                        .map(|fmmu| fmmu.sync_manager)
                        .unwrap_or(sync_manager_index);

                    let sm_config = SyncManagerChannel {
                        physical_start_address: sync_manager.start_addr,
                        length_bytes: byte_len,
                        control: sync_manager.control,
                        status: Default::default(),
                        enable: sync_manager_channel::Enable {
                            enable: sync_manager.enable.contains(SyncManagerEnable::ENABLE),
                            ..Default::default()
                        },
                    };

                    let fmmu_config = Fmmu {
                        logical_start_address: offset.start_address,
                        length_bytes: sm_config.length_bytes,
                        logical_start_bit: offset.start_bit,
                        logical_end_bit: offset.end_bit(bit_len),
                        physical_start_address: sm_config.physical_start_address,
                        physical_start_bit: 0x0,
                        read_enable: sync_manager.usage_type == SyncManagerType::ProcessDataIn,
                        write_enable: sync_manager.usage_type == SyncManagerType::ProcessDataOut,
                        enable: true,
                        reserved_1: 0,
                        reserved_2: 0,
                    };

                    self.client
                        .fpwr(
                            self.slave.configured_address,
                            RegisterAddress::sync_manager(sync_manager_index),
                            sm_config.pack().unwrap(),
                        )
                        .await?
                        .wkc(1, "SM")?;

                    // TODO: Maybe I need to set this in PRE-OP? It's failing on the AKD currently.
                    self.client
                        .fpwr(
                            self.slave.configured_address,
                            RegisterAddress::fmmu(fmmu_index),
                            fmmu_config.pack().unwrap(),
                        )
                        .await?
                        .wkc(1, "FMMU")?;

                    offset = offset.increment(bit_len);
                }
            }
        }

        Ok(offset)
    }
}

#[derive(Default, Debug, Copy, Clone, PartialEq, Eq)]
pub struct MappingOffset {
    start_address: u32,
    start_bit: u8,
}

impl MappingOffset {
    /// Increment, calculating values for _next_ mapping when the struct is read after increment.
    fn increment(self, bits: u16) -> Self {
        let mut inc_bytes = bits / 8;
        let inc_bits = bits % 8;

        // Bit count overflows a byte, so move into the next byte's bits by incrementing the byte
        // index one more.
        let start_bit = if u16::from(self.start_bit) + inc_bits >= 8 {
            inc_bytes += 1;

            ((u16::from(self.start_bit) + inc_bits) % 8) as u8
        } else {
            self.start_bit + inc_bits as u8
        };

        Self {
            start_address: self.start_address + u32::from(inc_bytes),
            start_bit,
        }
    }

    /// Compute end bit 0-7 in the final byte of the mapped PDI section.
    fn end_bit(self, bits: u16) -> u8 {
        // SAFETY: The modulos here and in `increment` mean that all value can comfortably fit in a
        // u8, so all the `as` and non-checked `+` here are fine.

        let bits = (bits.saturating_sub(1) % 8) as u8;

        let end = self.start_bit + bits % 8;

        end
    }

    fn size_bytes(self) -> usize {
        let size = self.start_address + (u32::from(self.start_bit) + 7) / 8;

        size as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn size_bytes() {
        // E.g. 2x EL2004, 1x EL1004
        let input = MappingOffset::default()
            .increment(4)
            .increment(4)
            .increment(4);

        assert_eq!(input.size_bytes(), 2);
    }

    #[test]
    fn simulate_2_el2004() {
        let input = MappingOffset::default();

        let input = input.increment(4);

        assert_eq!(
            input,
            MappingOffset {
                start_address: 0,
                start_bit: 4
            }
        );

        let input = input.increment(4);

        assert_eq!(
            input,
            MappingOffset {
                start_address: 1,
                start_bit: 0
            }
        );
    }

    #[test]
    fn end_bit() {
        let input = MappingOffset::default();

        assert_eq!(input.end_bit(4), 3);

        let input = input.increment(4);

        assert_eq!(input.end_bit(4), 7);

        let input = input.increment(4);

        assert_eq!(input.end_bit(4), 3);
    }

    #[test]
    fn zero_length_end_bit() {
        let input = MappingOffset::default();

        assert_eq!(input.end_bit(0), 0);

        let input = input.increment(4);

        assert_eq!(input.end_bit(0), 4);
    }

    #[test]
    fn cross_boundary() {
        let input = MappingOffset::default();

        let input = input.increment(6);

        assert_eq!(
            input,
            MappingOffset {
                start_address: 0,
                start_bit: 6
            }
        );

        let input = input.increment(6);

        assert_eq!(
            input,
            MappingOffset {
                start_address: 1,
                start_bit: 4
            }
        );
    }
}
