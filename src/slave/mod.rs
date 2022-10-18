mod configuration;
mod sdo;
pub mod slave_client;

use self::slave_client::SlaveClient;
use crate::{
    al_control::AlControl,
    al_status_code::AlStatusCode,
    all_consumed,
    client::Client,
    eeprom::{
        types::{MailboxProtocols, SiiOwner},
        Eeprom,
    },
    error::Error,
    pdi::PdiSegment,
    pdu_data::{PduData, PduRead},
    pdu_loop::CheckWorkingCounter,
    register::RegisterAddress,
    slave_state::SlaveState,
    timer_factory::TimerFactory,
};
use core::{
    fmt,
    fmt::{Debug, Write},
    time::Duration,
};
use nom::{number::complete::le_u32, IResult};
use packed_struct::PackedStruct;

#[derive(Default)]
pub struct SlaveIdentity {
    pub vendor_id: u32,
    pub product_id: u32,
    pub revision: u32,
    pub serial: u32,
}

impl fmt::Debug for SlaveIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SlaveIdentity")
            .field("vendor_id", &format_args!("{:#010x}", self.vendor_id))
            .field("product_id", &format_args!("{:#010x}", self.product_id))
            .field("revision", &self.revision)
            .field("serial", &self.serial)
            .finish()
    }
}

impl SlaveIdentity {
    pub fn parse(i: &[u8]) -> IResult<&[u8], Self> {
        let (i, vendor_id) = le_u32(i)?;
        let (i, product_id) = le_u32(i)?;
        let (i, revision) = le_u32(i)?;
        let (i, serial) = le_u32(i)?;

        all_consumed(i)?;

        Ok((
            i,
            Self {
                vendor_id,
                product_id,
                revision,
                serial,
            },
        ))
    }
}

#[derive(Debug, Default, Clone)]
pub struct SlaveConfig {
    pub io: IoRanges,
    pub mailbox: MailboxConfig,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct MailboxConfig {
    read: Option<Mailbox>,
    write: Option<Mailbox>,
    supported_protocols: MailboxProtocols,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct Mailbox {
    address: u16,
    len: u16,
    sync_manager: u8,
}

#[derive(Debug, Default, Clone)]
pub struct IoRanges {
    pub input: Option<PdiSegment>,
    pub output: Option<PdiSegment>,
}

impl IoRanges {
    pub fn working_counter_sum(&self) -> u16 {
        self.input.as_ref().map(|_| 1).unwrap_or(0) + self.output.as_ref().map(|_| 2).unwrap_or(0)
    }
}

#[derive(Debug)]
pub struct Slave {
    /// Configured station address.
    pub(crate) configured_address: u16,

    pub(crate) config: SlaveConfig,

    pub identity: SlaveIdentity,

    // NOTE: Default length in SOEM is 40 bytes
    pub name: heapless::String<64>,
}

impl Slave {
    pub(crate) async fn new<'client, const MAX_FRAMES: usize, const MAX_PDU_DATA: usize, TIMEOUT>(
        client: &'client Client<'client, MAX_FRAMES, MAX_PDU_DATA, TIMEOUT>,
        configured_address: u16,
    ) -> Result<Self, Error>
    where
        TIMEOUT: TimerFactory,
    {
        let mut config = SlaveConfig::default();

        let slave_ref = SlaveRef::new(client, &mut config, configured_address, "");

        slave_ref.wait_for_state(SlaveState::Init).await?;

        // Will be/should be set to SiiOwner::Pdi after init
        slave_ref.set_eeprom_mode(SiiOwner::Master).await?;

        let eep = slave_ref.eeprom();

        let identity = eep.identity().await?;

        let name = eep.device_name().await?.unwrap_or_else(|| {
            let mut s = heapless::String::new();

            write!(
                s,
                "manu. {:#010x}, device {:#010x}, serial {:#010x}",
                identity.vendor_id, identity.product_id, identity.serial
            )
            .unwrap();

            s
        });

        log::debug!("Slave name {}", name);

        Ok(Self {
            configured_address,
            identity,
            name,
            config,
        })
    }

    pub(crate) fn io_segments(&self) -> &IoRanges {
        &self.config.io
    }
}

pub struct SlaveRef<'a, const MAX_FRAMES: usize, const MAX_PDU_DATA: usize, TIMEOUT> {
    client: SlaveClient<'a, MAX_FRAMES, MAX_PDU_DATA, TIMEOUT>,
    pub(crate) config: &'a mut SlaveConfig,
    configured_address: u16,
    name: &'a str,
}

impl<'a, const MAX_FRAMES: usize, const MAX_PDU_DATA: usize, TIMEOUT>
    SlaveRef<'a, MAX_FRAMES, MAX_PDU_DATA, TIMEOUT>
where
    TIMEOUT: TimerFactory,
{
    pub fn new(
        client: &'a Client<MAX_FRAMES, MAX_PDU_DATA, TIMEOUT>,
        config: &'a mut SlaveConfig,
        configured_address: u16,
        name: &'a str,
    ) -> Self {
        Self {
            client: SlaveClient::new(client, configured_address),
            config,
            configured_address,
            name,
        }
    }

    pub fn name(&self) -> &str {
        self.name
    }

    async fn wait_for_state(&self, desired_state: SlaveState) -> Result<(), Error> {
        crate::timeout::<TIMEOUT, _, _>(Duration::from_millis(5000), async {
            loop {
                let status = self
                    .client
                    .read::<AlControl>(RegisterAddress::AlStatus, "Read AL status")
                    .await?;

                if status.state == desired_state {
                    break Result::<(), _>::Ok(());
                }

                TIMEOUT::timer(Duration::from_millis(10)).await;
            }
        })
        .await
    }

    pub async fn request_slave_state(&self, desired_state: SlaveState) -> Result<(), Error> {
        debug!(
            "Set state {} for slave address {:#04x}",
            desired_state, self.configured_address
        );

        // Send state request
        self.client
            .write(
                RegisterAddress::AlControl,
                AlControl::new(desired_state).pack().unwrap(),
                "AL control",
            )
            .await?;

        self.wait_for_state(desired_state).await
    }

    pub async fn status(&self) -> Result<(SlaveState, AlStatusCode), Error> {
        let status = self
            .client
            .read::<AlControl>(RegisterAddress::AlStatus, "AL Status")
            .await
            .map(|ctl| ctl.state)?;

        let code = self
            .client
            .read::<AlStatusCode>(RegisterAddress::AlStatusCode, "AL Status Code")
            .await?;

        Ok((status, code))
    }

    // TODO: Separate TIMEOUT for EEPROM specifically
    pub fn eeprom(&'a self) -> Eeprom<'a, MAX_FRAMES, MAX_PDU_DATA, TIMEOUT> {
        Eeprom::new(&self.client)
    }

    async fn set_eeprom_mode(&self, mode: SiiOwner) -> Result<(), Error> {
        self.client
            .write::<u16>(RegisterAddress::SiiConfig, 2, "debug write")
            .await?;
        self.client
            .write::<u16>(RegisterAddress::SiiConfig, mode as u16, "debug write 2")
            .await?;

        Ok(())
    }
}
