pub mod configurator;
pub mod ports;
pub mod slave_client;

use self::slave_client::SlaveClient;
use crate::{
    all_consumed,
    client::Client,
    coe::{self, abort_code::AbortCode, services::CoeServiceTrait, SubIndex},
    command::Command,
    dl_status::DlStatus,
    eeprom::types::{FromEeprom, MailboxProtocols, SiiOwner},
    error::{Error, MailboxError, PduError},
    mailbox::MailboxType,
    pdi::PdiSegment,
    pdu_data::{PduData, PduRead},
    pdu_loop::CheckWorkingCounter,
    register::{RegisterAddress, SupportFlags},
    slave::ports::{Port, Ports},
    slave_state::SlaveState,
    sync_manager_channel::SyncManagerChannel,
    timer_factory::TimerFactory,
};
use core::{
    any::type_name,
    fmt::{self, Debug, Write},
};
use nom::{bytes::complete::take, number::complete::le_u32, IResult};
use num_enum::TryFromPrimitive;
use packed_struct::{PackedStruct, PackedStructSlice};

#[derive(Default, Copy, Clone)]
pub struct SlaveIdentity {
    pub vendor_id: u32,
    pub product_id: u32,
    pub revision: u32,
    pub serial: u32,
}

impl Debug for SlaveIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SlaveIdentity")
            .field("vendor_id", &format_args!("{:#010x}", self.vendor_id))
            .field("product_id", &format_args!("{:#010x}", self.product_id))
            .field("revision", &self.revision)
            .field("serial", &self.serial)
            .finish()
    }
}

impl FromEeprom for SlaveIdentity {
    const STORAGE_SIZE: usize = 16;

    fn parse_fields(i: &[u8]) -> IResult<&[u8], Self> {
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
    pub input: PdiSegment,
    pub output: PdiSegment,
}

impl IoRanges {
    /// Expected working counter value for this slave.
    ///
    /// The working counter is calculated as follows:
    ///
    /// - If the slave has input data, increment by 1
    /// - If the slave has output data, increment by 2
    pub fn working_counter_sum(&self) -> u16 {
        let l = self.input.len().min(1) + (self.output.len().min(1) * 2);

        l as u16
    }
}

#[derive(Debug, Clone)]
pub struct Slave {
    /// Configured station address.
    pub(crate) configured_address: u16,

    pub(crate) config: SlaveConfig,

    pub identity: SlaveIdentity,

    // NOTE: Default length in SOEM is 40 bytes
    pub name: heapless::String<64>,

    pub(crate) flags: SupportFlags,

    pub(crate) ports: Ports,

    /// Distributed Clock latch receive time.
    pub(crate) dc_receive_time: i64,

    /// The index of the slave in the EtherCAT tree.
    pub(crate) index: usize,

    /// The index of the previous slave in the EtherCAT tree.
    ///
    /// For the first slave in the network, this will always be `None`.
    pub(crate) parent_index: Option<usize>,

    /// Propagation delay in nanoseconds.
    ///
    /// `u32::MAX` gives a maximum propagation delay of ~4.2 seconds for the last slave in the
    /// network.
    pub(crate) propagation_delay: u32,
}

impl Slave {
    /// Create a slave instance using the given configured address.
    ///
    /// This method reads the slave's name and other identifying information, but does not configure
    /// the slave.
    pub(crate) async fn new<'client, TIMEOUT>(
        client: &'client Client<'client, TIMEOUT>,
        index: usize,
        configured_address: u16,
    ) -> Result<Self, Error>
    where
        TIMEOUT: TimerFactory,
    {
        let slave_ref = SlaveClient::new(client, configured_address);

        slave_ref.wait_for_state(SlaveState::Init).await?;

        // Make sure master has access to slave EEPROM
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

        let flags = slave_ref
            .read::<SupportFlags>(RegisterAddress::SupportFlags, "support flags")
            .await?;

        let ports = slave_ref
            .read::<DlStatus>(RegisterAddress::DlStatus, "DL status")
            .await
            .map(|dl_status| {
                // NOTE: dc_receive_times are populated during DC initialisation
                Ports([
                    Port {
                        number: 0,
                        active: dl_status.link_port0,
                        ..Default::default()
                    },
                    Port {
                        number: 1,
                        active: dl_status.link_port1,
                        ..Default::default()
                    },
                    Port {
                        number: 2,
                        active: dl_status.link_port2,
                        ..Default::default()
                    },
                    Port {
                        number: 3,
                        active: dl_status.link_port3,
                        ..Default::default()
                    },
                ])
            })?;

        log::debug!("Slave {:#06x} name {}", configured_address, name);

        Ok(Self {
            configured_address,
            identity,
            name,
            config: SlaveConfig::default(),
            flags,
            index,
            parent_index: None,
            ports,
            propagation_delay: 0,
            dc_receive_time: 0,
        })
    }

    pub(crate) fn io_segments(&self) -> &IoRanges {
        &self.config.io
    }
}

#[derive(Debug)]
pub struct SlaveRef<'a, TIMEOUT> {
    client: SlaveClient<'a, TIMEOUT>,
    slave: &'a Slave,
}

impl<'a, TIMEOUT> SlaveRef<'a, TIMEOUT>
where
    TIMEOUT: TimerFactory,
{
    pub fn new(client: SlaveClient<'a, TIMEOUT>, slave: &'a Slave) -> Self {
        Self { client, slave }
    }

    pub fn name(&self) -> &str {
        self.slave.name.as_str()
    }

    pub async fn state(&self) -> Result<SlaveState, Error> {
        let (state, _code) = self.client.status().await?;

        Ok(state)
    }

    pub async fn write_sdo<T>(&self, index: u16, sub_index: SubIndex, value: T) -> Result<(), Error>
    where
        T: PduData,
        <T as PduRead>::Error: Debug,
    {
        let counter = self.client.mailbox_counter();

        if T::len() > 4 {
            // TODO: Normal SDO download. Only expedited requests for now
            panic!("Data too long");
        }

        let mut data = [0u8; 4];

        let len = usize::from(T::len());

        data[0..len].copy_from_slice(value.as_slice());

        let request = coe::services::download(counter, index, sub_index, data, len as u8);

        let (_response, _data) = self.send_coe_service(request).await?;

        // TODO: Validate reply?

        Ok(())
    }

    pub async fn read_sdo<T>(&self, index: u16, sub_index: SubIndex) -> Result<T, Error>
    where
        T: PduData,
        <T as PduRead>::Error: Debug,
    {
        // FIXME: Make this dynamic somehow
        let mut buf = [0u8; 32];

        self.read_sdo_buf(index, sub_index, &mut buf)
            .await
            .and_then(|data| {
                T::try_from_slice(data).map_err(|_| {
                    log::error!(
                        "SDO expedited data decode T: {} (len {}) data {:?} (len {})",
                        type_name::<T>(),
                        T::len(),
                        data,
                        data.len()
                    );

                    Error::Pdu(PduError::Decode)
                })
            })
    }

    /// Read a register.
    ///
    /// Note that while this method is marked safe, alterations to slave config or behaviour can
    /// break interactions with EtherCrab.
    pub async fn raw_read<T>(&self, register: RegisterAddress) -> Result<T, Error>
    where
        T: PduRead,
        <T as PduRead>::Error: Debug,
    {
        self.client.read(register, "raw read").await
    }

    /// Write a register.
    ///
    /// Note that while this method is marked safe, alterations to slave config or behaviour can
    /// break interactions with EtherCrab.
    pub async fn raw_write<T>(&self, register: impl Into<u16>, value: T) -> Result<T, Error>
    where
        T: PduData,
        <T as PduRead>::Error: Debug,
    {
        self.client.write(register, value, "raw write").await
    }

    async fn read_sdo_buf<'buf>(
        &self,
        index: u16,
        sub_index: SubIndex,
        buf: &'buf mut [u8],
    ) -> Result<&'buf [u8], Error> {
        let request = coe::services::upload(self.client.mailbox_counter(), index, sub_index);

        let (headers, data) = self.send_coe_service(request).await?;

        // Expedited transfers where the data is 4 bytes or less long, denoted in the SDO header
        // size value.
        if headers.sdo_header.flags.expedited_transfer {
            let data_len = 4usize.saturating_sub(usize::from(headers.sdo_header.flags.size));
            let data = &data[0..data_len];

            let buf = &mut buf[0..data_len];

            buf.copy_from_slice(data);

            Ok(buf)
        }
        // Data is either a normal upload or a segmented upload
        else {
            let data_length = headers.header.length.saturating_sub(0x0a);

            let (data, complete_size) = le_u32(data)?;

            // The provided buffer isn't long enough to contain all mailbox data.
            if complete_size > buf.len() as u32 {
                return Err(Error::Mailbox(MailboxError::TooLong {
                    address: request.address(),
                    sub_index: request.sub_index(),
                }));
            }

            // If it's a normal upload, the response payload is returned in the initial mailbox read
            if complete_size <= u32::from(data_length) {
                let (_rest, data) = take(data_length)(data)?;

                buf.copy_from_slice(data);

                Ok(&buf[0..usize::from(data_length)])
            }
            // If it's a segmented upload, we must make subsequent requests to load all segment data
            // from the read mailbox.
            else {
                let mut toggle = false;
                let mut total_len = 0usize;

                loop {
                    let request =
                        coe::services::upload_segmented(self.client.mailbox_counter(), toggle);

                    let (headers, data) = self.send_coe_service(request).await?;

                    // The spec defines the data length as n-3, so we'll just go with that magic
                    // number...
                    let mut chunk_len = usize::from(headers.header.length - 3);

                    // Special case as per spec: Minimum response size is 7 bytes. For smaller
                    // responses, we must remove the number of unused bytes at the end of the
                    // response. Extremely weird.
                    if chunk_len == 7 {
                        chunk_len -= usize::from(headers.sdo_header.segment_data_size);
                    }

                    let data = &data[0..chunk_len];

                    buf[total_len..][..chunk_len].copy_from_slice(data);
                    total_len += chunk_len;

                    if headers.sdo_header.is_last_segment {
                        break;
                    }

                    toggle = !toggle;
                }

                Ok(&buf[0..total_len])
            }
        }
    }

    /// Send a mailbox request, wait for response mailbox to be ready, read response from mailbox
    /// and return as a slice.
    async fn send_coe_service<H>(&self, request: H) -> Result<(H, &[u8]), Error>
    where
        H: CoeServiceTrait + packed_struct::PackedStructInfo,
        <H as PackedStruct>::ByteArray: AsRef<[u8]>,
    {
        let write_mailbox = self
            .slave
            .config
            .mailbox
            .write
            .ok_or(Error::Mailbox(MailboxError::NoMailbox))?;
        let read_mailbox = self
            .slave
            .config
            .mailbox
            .read
            .ok_or(Error::Mailbox(MailboxError::NoMailbox))?;

        let counter = request.counter();

        // TODO: Abstract this into a method that returns a slice
        self.client
            .client
            .pdu_loop
            .pdu_tx_readwrite_len(
                Command::Fpwr {
                    address: self.slave.configured_address,
                    register: write_mailbox.address,
                },
                request.pack().unwrap().as_ref(),
                write_mailbox.len,
            )
            .await?
            .wkc(1, "SDO upload request")?;

        // Wait for slave send mailbox to be ready
        crate::timer_factory::timeout::<TIMEOUT, _, _>(
            self.client.timeouts().mailbox_echo,
            async {
                let mailbox_read_sm = RegisterAddress::sync_manager(read_mailbox.sync_manager);

                loop {
                    let sm = self
                        .client
                        .read::<SyncManagerChannel>(mailbox_read_sm, "Master read mailbox")
                        .await?;

                    if sm.status.mailbox_full {
                        break Ok(());
                    }

                    self.client.timeouts().loop_tick::<TIMEOUT>().await;
                }
            },
        )
        .await
        .map_err(|e| {
            log::error!("Mailbox read ready error: {e:?}");

            e
        })?;

        // Receive data from slave send mailbox
        // TODO: Abstract this into a method that returns a slice
        let response = self
            .client
            .client
            .pdu_loop
            .pdu_tx_readonly(
                Command::Fprd {
                    address: self.slave.configured_address,
                    register: read_mailbox.address,
                },
                read_mailbox.len,
            )
            .await?
            .wkc(1, "SDO read mailbox")?;

        // TODO: Retries. Refer to SOEM's `ecx_mbxreceive` for inspiration

        let headers_len = H::packed_bits() / 8;

        let (headers, data) = response.split_at(headers_len);

        let headers = H::unpack_from_slice(headers).map_err(|e| {
            log::error!("Failed to unpack mailbox response headers: {e}");

            e
        })?;

        if headers.is_aborted() {
            let code = data[0..4]
                .try_into()
                .map_err(|_| ())
                .and_then(|arr| {
                    AbortCode::try_from_primitive(u32::from_le_bytes(arr)).map_err(|_| ())
                })
                .unwrap_or(AbortCode::General);

            Err(Error::Mailbox(MailboxError::Aborted {
                code,
                address: request.address(),
                sub_index: request.sub_index(),
            }))
        }
        // Validate that the mailbox response is to the request we just sent
        // TODO: Determine if we need to check the counter. I don't think SOEM does, it might just
        // be used by the slave?
        else if headers.mailbox_type() != MailboxType::Coe
        /* || headers.counter() != counter */
        {
            log::error!(
                "Invalid SDO response. Type: {:?} (expected {:?}), counter {} (expected {})",
                headers.mailbox_type(),
                MailboxType::Coe,
                headers.counter(),
                counter
            );

            Err(Error::Mailbox(MailboxError::SdoResponseInvalid {
                address: request.address(),
                sub_index: request.sub_index(),
            }))
        } else {
            Ok((headers, data))
        }
    }
}
