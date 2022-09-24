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
        offset: MappingOffset,
    ) -> Result<MappingOffset, Error> {
        // TODO: Check if mailbox is supported or not; autoconfig is different if it is.

        // RX from the perspective of the slave device
        let rx_pdos = self.eeprom().rxpdos().await?;

        let sync_managers = self.eeprom().sync_managers().await?;

        // dbg!(&rx_pdos);

        // TODO: No fixed index
        let index = 0;

        // NOTE: Fixed index of zero is an output SM if mailbox is not supported
        // TODO: In light of this, use `eeprom().fmmus()` to decide what op mode each SM/FMMU should have
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

            let byte_len = u16::from((bit_len + 7) / 8);

            log::debug!("Sync manager {index} has bit length {bit_len}");

            // TODO: What happens if bit_len is zero?

            let tx_config = SyncManagerChannel {
                physical_start_address: write_sm.start_addr,
                length_bytes: byte_len,
                control: write_sm.control,
                status: Default::default(),
                enable: sync_manager_channel::Enable {
                    enable: write_sm.enable.contains(SyncManagerEnable::ENABLE),
                    ..Default::default()
                },
            };

            let fmmu_config = Fmmu {
                logical_start_address: offset.start_address,
                length_bytes: tx_config.length_bytes,
                logical_start_bit: offset.start_bit,
                logical_end_bit: offset.end_bit(bit_len),
                physical_start_address: tx_config.physical_start_address,
                physical_start_bit: 0x0,
                read_enable: false,
                write_enable: true,
                enable: true,
                reserved_1: 0,
                reserved_2: 0,
            };

            dbg!(fmmu_config);

            self.client
                .fpwr(
                    self.slave.configured_address,
                    RegisterAddress::Sm0,
                    tx_config.pack().unwrap(),
                )
                .await?
                .wkc(1, "SM0")?;

            self.client
                .fpwr(
                    self.slave.configured_address,
                    RegisterAddress::Fmmu0,
                    fmmu_config.pack().unwrap(),
                )
                .await?
                .wkc(1, "FMMU0")?;

            Ok(offset.increment(bit_len))
        } else {
            Ok(offset)
        }
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
}

#[cfg(test)]
mod tests {
    use super::*;

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
