use crate::{
    al_control::AlControl,
    al_status::AlState,
    al_status_code::AlStatusCode,
    client::Client,
    eeprom::{
        types::{FmmuUsage, SyncManagerEnable, SyncManagerType},
        Eeprom,
    },
    error::Error,
    fmmu::Fmmu,
    pdu::CheckWorkingCounter,
    register::RegisterAddress,
    sync_manager_channel::{self, SyncManagerChannel},
    timer_factory::TimerFactory,
    PduData, PduRead,
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

    /// A wrapper around an FPRD service to this slave's configured address.
    pub(crate) async fn read<T>(
        &self,
        register: RegisterAddress,
        context: &'static str,
    ) -> Result<T, Error>
    where
        T: PduRead,
    {
        self.client
            .fprd(self.configured_address, register)
            .await?
            .wkc(1, context)
    }

    /// A wrapper around an FPWR service to this slave's configured address.
    pub(crate) async fn write<T>(
        &self,
        register: RegisterAddress,
        value: T,
        context: &'static str,
    ) -> Result<T, Error>
    where
        T: PduData,
    {
        self.client
            .fpwr(self.configured_address, register, value)
            .await?
            .wkc(1, context)
    }

    pub async fn request_slave_state(&self, state: AlState) -> Result<(), Error> {
        debug!(
            "Set state {} for slave address {:#04x}",
            state, self.slave.configured_address
        );

        // Send state request
        self.write(
            RegisterAddress::AlControl,
            AlControl::new(state).pack().unwrap(),
            "AL control",
        )
        .await?;

        let res = crate::timeout::<TIMEOUT, _, _>(Duration::from_millis(1000), async {
            loop {
                let status = self
                    .read::<AlControl>(RegisterAddress::AlStatus, "Read AL status")
                    .await?;

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
                    let status = self
                        .read::<AlStatusCode>(RegisterAddress::AlStatusCode, "Read AL status")
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
        Eeprom::new(&self)
    }

    /// Configuration performed in `INIT` state.
    pub async fn configure_from_eeprom_init(&self) -> Result<(), Error> {
        // TODO: Check if mailbox is supported or not; autoconfig is different if it is.

        // TODO: Cleanup; only force EEPROM into normal mode
        {
            let stuff = self
                .read::<u16>(RegisterAddress::SiiConfig, "debug read")
                .await?;

            log::info!("CHECK {:016b}", stuff);

            // Force owner away from PDI so we can read it over the EtherCAT DL.
            self.write::<u16>(RegisterAddress::SiiConfig, 2, "debug write")
                .await?;
            self.write::<u16>(RegisterAddress::SiiConfig, 0, "debug write 2")
                .await?;
        }

        let sync_managers = self.eeprom().sync_managers().await?;

        log::trace!("sync_managers {:#?}", sync_managers);

        for (sync_manager_index, sync_manager) in sync_managers.iter().enumerate() {
            let sync_manager_index = sync_manager_index as u8;

            match sync_manager.usage_type {
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

                    self.write(
                        RegisterAddress::sync_manager(sync_manager_index),
                        sm_config.pack().unwrap(),
                        "Mailbox SM",
                    )
                    .await?;

                    log::debug!("SM{sync_manager_index} {:#?}", sm_config);
                }
                _ => continue,
            }
        }

        // TODO: Cleanup; according to SOEM "some slaves need eeprom available to PDI in init->preop transition"
        {
            // Force EEPROM into PDI mode for some slaves.
            self.write::<u16>(RegisterAddress::SiiConfig, 1, "debug write")
                .await?;

            let stuff = self
                .read::<u16>(RegisterAddress::SiiConfig, "debug read")
                .await?;

            log::info!("CHECK2 {:016b}", stuff);

            self.request_slave_state(AlState::PreOp).await?;
        }

        Ok(())
    }

    // TODO: PO2SO callback for configuring SDOs
    // TODO: Lots of dedupe with configure_from_eeprom_init
    /// Configure slave in `PRE-OP` state.
    pub async fn configure_from_eeprom_preop(
        &self,
        mut offset: MappingOffset,
    ) -> Result<MappingOffset, Error> {
        // Force EEPROM back into master mode.
        // Looks like SOEM set it to PDI just for INIT -> PRE-OP transition, then sets it back to
        // master again during the next EEPROM read.
        {
            let stuff = self
                .read::<u16>(RegisterAddress::SiiConfig, "debug read")
                .await?;

            log::info!("CHECK {:016b}", stuff);

            // Force owner away from PDI to master mode so we can read it over the EtherCAT DL.
            self.write::<u16>(RegisterAddress::SiiConfig, 2, "debug write")
                .await?;
            self.write::<u16>(RegisterAddress::SiiConfig, 0, "debug write 2")
                .await?;
        }

        // RX from the perspective of the slave device
        let rx_pdos = self.eeprom().rxpdos().await?;
        let tx_pdos = self.eeprom().txpdos().await?;

        let sync_managers = self.eeprom().sync_managers().await?;
        let fmmu_usage = self.eeprom().fmmus().await?;
        let fmmu_sm_mappings = self.eeprom().fmmu_mappings().await?;

        for (sync_manager_index, sync_manager) in sync_managers.iter().enumerate() {
            let sync_manager_index = sync_manager_index as u8;

            match sync_manager.usage_type {
                SyncManagerType::ProcessDataWrite | SyncManagerType::ProcessDataRead => {
                    let pdos = if sync_manager.usage_type == SyncManagerType::ProcessDataWrite {
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

                    // Look for FMMU index using FMMU_EX section in EEPROM. If it's empty, default
                    // to looking through FMMU usage list and picking out the appropriate kind
                    // (Inputs, Outputs)
                    let fmmu_index = fmmu_sm_mappings
                        .iter()
                        .find(|fmmu| fmmu.sync_manager == sync_manager_index)
                        .map(|fmmu| fmmu.sync_manager)
                        .or_else(|| {
                            log::trace!("Could not find FMMU for PDO SM{sync_manager_index}");

                            fmmu_usage
                                .iter()
                                .position(|usage| match (sync_manager.usage_type, usage) {
                                    (SyncManagerType::ProcessDataWrite, FmmuUsage::Outputs) => true,
                                    (SyncManagerType::ProcessDataRead, FmmuUsage::Inputs) => true,
                                    _ => false,
                                })
                                .map(|idx| {
                                    log::trace!("Using fallback FMMU FMMU{idx}");

                                    idx as u8
                                })
                        })
                        .ok_or(Error::Other)?;

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

                    log::debug!("SM{sync_manager_index} {:#?}", sm_config);

                    let fmmu_config = Fmmu {
                        logical_start_address: offset.start_address,
                        length_bytes: sm_config.length_bytes,
                        logical_start_bit: offset.start_bit,
                        logical_end_bit: offset.end_bit(bit_len),
                        physical_start_address: sm_config.physical_start_address,
                        physical_start_bit: 0x0,
                        read_enable: sync_manager.usage_type == SyncManagerType::ProcessDataRead,
                        write_enable: sync_manager.usage_type == SyncManagerType::ProcessDataWrite,
                        enable: true,
                        reserved_1: 0,
                        reserved_2: 0,
                    };

                    log::debug!("FMMU{fmmu_index} {:#?}", fmmu_config);

                    self.write(
                        RegisterAddress::sync_manager(sync_manager_index),
                        sm_config.pack().unwrap(),
                        "PDI SM",
                    )
                    .await?;

                    self.write(
                        RegisterAddress::fmmu(fmmu_index),
                        fmmu_config.pack().unwrap(),
                        "PDI FMMU",
                    )
                    .await?;

                    offset = offset.increment(bit_len);
                }
                _ => continue,
            }
        }

        // Put EEPROM into PDI mode again
        // TODO: Figure out why I need this beyond "SOEM does it too"
        {
            self.write::<u16>(RegisterAddress::SiiConfig, 1, "debug write")
                .await?;

            let stuff = self
                .read::<u16>(RegisterAddress::SiiConfig, "debug read")
                .await?;

            log::info!("CHECK2 {:016b}", stuff);

            self.request_slave_state(AlState::SafeOp).await?;
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

        self.start_bit + bits % 8
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
