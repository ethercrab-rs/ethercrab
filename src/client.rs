use crate::{
    al_control::AlControl,
    command::Command,
    dl_status::DlStatus,
    error::{Error, Item, PduError},
    pdi::PdiOffset,
    pdu_data::{PduData, PduRead},
    pdu_loop::{CheckWorkingCounter, PduLoop, PduResponse},
    register::{PortDescriptors, RegisterAddress, SupportFlags},
    slave::{slave_client::SlaveClient, Slave},
    slave_group::SlaveGroupContainer,
    slave_state::SlaveState,
    timer_factory::{Timeouts, TimerFactory},
    BASE_SLAVE_ADDR,
};
use core::{
    any::type_name,
    cell::RefCell,
    marker::PhantomData,
    sync::atomic::{AtomicU8, Ordering},
};
use packed_struct::PackedStruct;

pub struct Client<'client, const MAX_FRAMES: usize, const MAX_PDU_DATA: usize, TIMEOUT> {
    // TODO: un-pub
    // TODO: Experiment with taking a dyn trait reference
    pub pdu_loop: &'client PduLoop<MAX_FRAMES, MAX_PDU_DATA, TIMEOUT>,
    num_slaves: RefCell<u16>,
    _timeout: PhantomData<TIMEOUT>,
    pub(crate) timeouts: Timeouts,
    /// The 1-7 cyclic counter used when working with mailbox requests.
    mailbox_counter: AtomicU8,
}

unsafe impl<'client, const MAX_FRAMES: usize, const MAX_PDU_DATA: usize, TIMEOUT> Sync
    for Client<'client, MAX_FRAMES, MAX_PDU_DATA, TIMEOUT>
{
}

impl<'client, const MAX_FRAMES: usize, const MAX_PDU_DATA: usize, TIMEOUT>
    Client<'client, MAX_FRAMES, MAX_PDU_DATA, TIMEOUT>
where
    TIMEOUT: TimerFactory,
{
    pub fn new(
        pdu_loop: &'client PduLoop<MAX_FRAMES, MAX_PDU_DATA, TIMEOUT>,
        timeouts: Timeouts,
    ) -> Self {
        // MSRV: Make `MAX_FRAMES` a `u8` when `generic_const_exprs` is stablised
        assert!(
            MAX_FRAMES <= u8::MAX.into(),
            "Packet indexes are u8s, so cache array cannot be any bigger than u8::MAX"
        );

        Self {
            pdu_loop,
            // slaves: UnsafeCell::new(heapless::Vec::new()),
            num_slaves: RefCell::new(0),
            _timeout: PhantomData,
            timeouts,
            // 0 is a reserved value, so we initialise the cycle at 1. The cycle repeats 1 - 7.
            mailbox_counter: AtomicU8::new(1),
        }
    }

    /// Return the current cyclic mailbox counter value, from 0-7.
    ///
    /// Calling this method internally increments the counter, so subequent calls will produce a new
    /// value.
    pub(crate) fn mailbox_counter(&self) -> u8 {
        self.mailbox_counter
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| {
                if n >= 7 {
                    Some(1)
                } else {
                    Some(n + 1)
                }
            })
            .unwrap()
    }

    /// Write zeroes to every slave's memory in chunks of [`MAX_PDU_DATA`].
    async fn blank_memory(&self, start: impl Into<u16>, len: u16) -> Result<(), Error> {
        let start: u16 = start.into();
        let step = MAX_PDU_DATA;
        let range = start..(start + len);

        for chunk_start in range.step_by(step) {
            self.write_service(
                Command::Bwr {
                    address: 0,
                    register: chunk_start,
                },
                [0u8; MAX_PDU_DATA],
            )
            .await?;
        }

        Ok(())
    }

    async fn reset_slaves(&self) -> Result<(), Error> {
        // Reset slaves to init
        self.bwr(
            RegisterAddress::AlControl,
            AlControl::reset().pack().unwrap(),
        )
        .await?;

        // Clear FMMUs. FMMU memory section is 0xff (255) bytes long - see ETG1000.4 Table 57
        self.blank_memory(RegisterAddress::Fmmu0, 0xff).await?;

        // Clear SMs. SM memory section is 0x7f bytes long - see ETG1000.4 Table 59
        self.blank_memory(RegisterAddress::Sm0, 0x7f).await?;

        self.blank_memory(
            RegisterAddress::DcSystemTime,
            core::mem::size_of::<u64>() as u16,
        )
        .await?;

        Ok(())
    }

    /// Detect slaves and set their configured station addresses.
    pub async fn init<const MAX_SLAVES: usize, G>(
        &self,
        mut groups: G,
        mut group_filter: impl FnMut(&mut G, Slave) -> Result<(), Error>,
    ) -> Result<G, Error>
    where
        G: SlaveGroupContainer<MAX_FRAMES, MAX_PDU_DATA, TIMEOUT>,
    {
        self.reset_slaves().await?;

        // Each slave increments working counter, so we can use it as a total count of slaves
        let (_res, num_slaves) = self.brd::<u8>(RegisterAddress::Type).await?;

        *self.num_slaves.borrow_mut() = num_slaves;

        let mut slaves = heapless::Vec::<Slave, MAX_SLAVES>::new();

        // Set configured address for all discovered slaves
        for slave_idx in 0..num_slaves {
            let configured_address = BASE_SLAVE_ADDR.wrapping_add(slave_idx);

            self.apwr(
                slave_idx,
                RegisterAddress::ConfiguredStationAddress,
                configured_address,
            )
            .await?
            .wkc(1, "set station address")?;

            let slave = Slave::new(self, usize::from(slave_idx), configured_address).await?;

            slaves
                .push(slave)
                .map_err(|_| Error::Capacity(Item::Slave))?;
        }

        self.configure_dc(&mut slaves).await?;

        while let Some(slave) = slaves.pop() {
            let configured_address = slave.configured_address;
            let slave_name = slave.name.clone();

            let before_count = groups.total_slaves();

            group_filter(&mut groups, slave)?;

            if groups.total_slaves() != before_count + 1 {
                log::error!(
                    "Slave {:#06x} ({}) was not assigned to a group. All slaves must be assigned.",
                    configured_address,
                    slave_name
                );
            }
        }

        let mut offset = PdiOffset::default();

        // Loop through groups and configure the slaves in each one.
        for i in 0..groups.num_groups() {
            let mut group = groups.group(i).ok_or(Error::NotFound {
                item: Item::Group,
                index: Some(i),
            })?;

            offset = group.configure_from_eeprom(offset, self).await?;

            log::debug!("After group #{i} offset: {:?}", offset);
        }

        self.wait_for_state(SlaveState::SafeOp).await?;

        Ok(groups)
    }

    async fn configure_dc(&self, slaves: &mut [Slave]) -> Result<(), Error> {
        let num_slaves = slaves.len();

        // TODO: Read from slave list flags.dc_supported
        let first_dc_supported_slave = 0x1000;

        self.bwr(RegisterAddress::DcTimePort0, 0u32)
            .await
            .expect("Broadcast time")
            .wkc(num_slaves as u16, "Broadcast time")
            .unwrap();

        // let ethercat_offset = Utc.ymd(2000, 01, 01).and_hms(0, 0, 0);

        // let now_nanos =
        //     chrono::Utc::now().timestamp_nanos() - dbg!(ethercat_offset.timestamp_nanos());

        let now_nanos = 0;

        #[derive(Debug, Default, Copy, Clone)]
        struct PortStuff {
            number: u8,
            active: bool,
            dc_time: u32,
        }

        let mut prev_slave_ports = [PortStuff::default(); 4];

        for i in 0..slaves.len() {
            let (prev, rest) = slaves.split_at_mut(i);
            let mut slave = rest.first_mut().ok_or(Error::Internal)?;

            log::info!("Slave {:#06x}", slave.configured_address);

            let sl = SlaveClient::new(self, slave.configured_address);

            let port_descriptors = sl
                .read(RegisterAddress::PortDescriptors, "Port descriptors")
                .await?;

            // log::info!("Slave {:#06x} ports: {:#?}", slave_addr, port_descriptors);

            // let dl_status = sl
            //     .read::<DlStatus>(RegisterAddress::DlStatus, "Supported flags")
            //     .await?;

            // dbg!(dl_status);

            let flags = sl
                .read::<SupportFlags>(RegisterAddress::SupportFlags, "Supported flags")
                .await?;

            let time_p0 = sl
                .read::<u32>(RegisterAddress::DcTimePort0, "DC time port 0")
                .await?;

            let receive_time_p0_nanos = sl
                .read::<i64>(RegisterAddress::DcReceiveTime, "Receive time P0")
                .await?;
            // .wkc(1, "Receive time P0")
            // .unwrap();

            // let offset = u64::try_from(now_nanos).expect("Why negative???") - receive_time_p0;
            let offset = -receive_time_p0_nanos + now_nanos;

            // dbg!(receive_time_p0_nanos, offset);

            // sl.write(RegisterAddress::DcSystemTimeOffset, offset)
            //     .await?;
            // Why does this sometimes fail with wkc = 0? Looks like it's when it doesn't have an output port?
            // .wkc(1, "Write offset")
            // .expect("Write offset");

            let time_p1 = sl
                .read::<u32>(RegisterAddress::DcTimePort1, "DC time port 1")
                .await?;

            let time_p2 = sl
                .read::<u32>(RegisterAddress::DcTimePort2, "DC time port 2")
                .await?;

            let time_p3 = sl
                .read::<u32>(RegisterAddress::DcTimePort3, "DC time port 3")
                .await?;

            let ports = [
                PortStuff {
                    number: 0,
                    dc_time: time_p0,
                    active: slave.ports.port0,
                },
                PortStuff {
                    number: 1,
                    dc_time: time_p1,
                    active: slave.ports.port1,
                },
                PortStuff {
                    number: 2,
                    dc_time: time_p2,
                    active: slave.ports.port2,
                },
                PortStuff {
                    number: 3,
                    dc_time: time_p3,
                    active: slave.ports.port3,
                },
            ];

            log::info!(
                "--> Port times: ({} [{}], {} [{}], {} [{}], {} [{}])",
                ports[0].dc_time,
                ports[0].active as u8,
                ports[1].dc_time,
                ports[1].active as u8,
                ports[2].dc_time,
                ports[2].active as u8,
                ports[3].dc_time,
                ports[3].active as u8
            );

            let active_ports = ports.iter().filter_map(|p| p.active.then_some(p.dc_time));

            let loop_propagation_time = active_ports
                .clone()
                .max()
                .map(|max| max - active_ports.min().unwrap())
                .filter(|t| *t > 0);

            log::info!("--> Transit time {loop_propagation_time:?} ns");

            // // Don't look for parent port for first slave; it doesn't have one (well, it's the
            // // master)
            // let propagation_delay = if i > 0 {
            //     /// Find the port on the parent slave this slave is connected to.
            //     ///
            //     /// Parent port order goes:
            //     ///
            //     /// 3 -> 1 -> 2 -> 0
            //     fn parent_port<'port>(ports: &'port [PortStuff; 4]) -> &'port PortStuff {
            //         let reordered = [&ports[3], &ports[1], &ports[2], &ports[0]];

            //         // SAFETY: If we're talking to this slave, the data MUST have come through a parent,
            //         // so there has to be at least one active port.
            //         reordered.iter().find(|port| port.active).unwrap()
            //     }

            //     // Entry port into the slave is port with lowest latched DC time
            //     let entry_port = ports
            //         .into_iter()
            //         .filter(|port| port.active)
            //         .min_by_key(|port| port.dc_time)
            //         .unwrap();

            //     // The port on the upstream slave this slave is connected to
            //     let upstream_port = parent_port(&prev_slave_ports);

            //     // dbg!(entry_port, upstream_port);

            //     log::info!(
            //         "--> Parent port {} -> this slave port {}",
            //         upstream_port.number,
            //         entry_port.number
            //     );

            //     // This is correct to SOEM. 720 or 720ns using two LAN9252
            //     (time_p1 - time_p0) / 2
            // } else {
            //     0
            // };

            // prev_slave_ports = ports;

            // log::info!(
            //     "Slave {:#06x} receive time: {} ns, propagation delay {propagation_delay} ns",
            //     slave_addr,
            //     receive_time_p0_nanos
            // );

            if !flags.has_64bit_dc {
                // TODO
                log::warn!("--> Slave uses seconds instead of ns?");
            }

            if !flags.dc_supported {
                continue;
            }
        }

        Ok(())
    }

    pub fn num_slaves(&self) -> usize {
        usize::from(*self.num_slaves.borrow())
    }

    /// Request the same state for all slaves.
    pub async fn request_slave_state(&self, desired_state: SlaveState) -> Result<(), Error> {
        let num_slaves = *self.num_slaves.borrow();

        self.bwr(
            RegisterAddress::AlControl,
            AlControl::new(desired_state).pack().unwrap(),
        )
        .await?
        .wkc(num_slaves as u16, "set all slaves state")?;

        self.wait_for_state(desired_state).await
    }

    pub async fn wait_for_state(&self, desired_state: SlaveState) -> Result<(), Error> {
        let num_slaves = *self.num_slaves.borrow();

        crate::timer_factory::timeout::<TIMEOUT, _, _>(self.timeouts.state_transition, async {
            loop {
                let status = self
                    .brd::<AlControl>(RegisterAddress::AlStatus)
                    .await?
                    .wkc(num_slaves as u16, "read all slaves state")?;
                if status.state == desired_state {
                    break Result::<(), Error>::Ok(());
                }

                self.timeouts.loop_tick::<TIMEOUT>().await;
            }
        })
        .await
    }

    async fn read_service<T>(&self, command: Command) -> Result<PduResponse<T>, Error>
    where
        T: PduRead,
    {
        let (data, working_counter) = self
            .pdu_loop
            .pdu_tx_readonly(command, T::len(), &self.timeouts)
            .await?;

        let res = T::try_from_slice(data).map_err(|e| {
            log::error!(
                "PDU data decode: {:?}, T: {} data {:?}",
                e,
                type_name::<T>(),
                data
            );

            PduError::Decode
        })?;

        Ok((res, working_counter))
    }

    // TODO: Support different I and O types; some things can return different data
    async fn write_service<T>(&self, command: Command, value: T) -> Result<PduResponse<T>, Error>
    where
        T: PduData,
    {
        let (data, working_counter) = self
            .pdu_loop
            .pdu_tx_readwrite(command, value.as_slice(), &self.timeouts)
            .await?;

        let res = T::try_from_slice(data).map_err(|e| {
            log::error!(
                "PDU data decode: {:?}, T: {} data {:?}",
                e,
                type_name::<T>(),
                data
            );

            PduError::Decode
        })?;

        Ok((res, working_counter))
    }

    pub async fn brd<T>(&self, register: RegisterAddress) -> Result<PduResponse<T>, Error>
    where
        T: PduRead,
    {
        self.read_service(Command::Brd {
            // Address is always zero when sent from master
            address: 0,
            register: register.into(),
        })
        .await
    }

    /// Broadcast write.
    pub async fn bwr<T>(
        &self,
        register: RegisterAddress,
        value: T,
    ) -> Result<PduResponse<()>, Error>
    where
        T: PduData,
    {
        self.write_service(
            Command::Bwr {
                address: 0,
                register: register.into(),
            },
            value,
        )
        .await
        .map(|(_, wkc)| ((), wkc))
    }

    /// Auto Increment Physical Read.
    pub async fn aprd<T>(
        &self,
        address: u16,
        register: RegisterAddress,
    ) -> Result<PduResponse<T>, Error>
    where
        T: PduRead,
    {
        self.read_service(Command::Aprd {
            address: 0u16.wrapping_sub(address),
            register: register.into(),
        })
        .await
    }

    /// Auto Increment Physical Write.
    pub async fn apwr<T>(
        &self,
        address: u16,
        register: RegisterAddress,
        value: T,
    ) -> Result<PduResponse<T>, Error>
    where
        T: PduData,
    {
        self.write_service(
            Command::Apwr {
                address: 0u16.wrapping_sub(address),
                register: register.into(),
            },
            value,
        )
        .await
    }

    /// Configured address read.
    pub async fn fprd<T>(
        &self,
        address: u16,
        register: RegisterAddress,
    ) -> Result<PduResponse<T>, Error>
    where
        T: PduRead,
    {
        self.read_service(Command::Fprd {
            address,
            register: register.into(),
        })
        .await
    }

    /// Configured address write.
    pub async fn fpwr<T>(
        &self,
        address: u16,
        register: impl Into<u16>,
        value: T,
    ) -> Result<PduResponse<T>, Error>
    where
        T: PduData,
    {
        self.write_service(
            Command::Fpwr {
                address,
                register: register.into(),
            },
            value,
        )
        .await
    }

    /// Logical write.
    pub async fn lwr<T>(&self, address: u32, value: T) -> Result<PduResponse<T>, Error>
    where
        T: PduData,
    {
        self.write_service(Command::Lwr { address }, value).await
    }

    /// Logical read/write.
    pub async fn lrw<T>(&self, address: u32, value: T) -> Result<PduResponse<T>, Error>
    where
        T: PduData,
    {
        self.write_service(Command::Lrw { address }, value).await
    }

    /// Logical read/write, but direct from/to a mutable slice.
    // TODO: Chunked sends if buffer is too long for MAX_PDU_DATA
    pub async fn lrw_buf<'buf>(
        &self,
        address: u32,
        value: &'buf mut [u8],
    ) -> Result<PduResponse<&'buf mut [u8]>, Error> {
        assert!(value.len() <= MAX_PDU_DATA, "Chunked LRW not yet supported. Buffer of length {} is too long to send in one {} frame",value.len(), MAX_PDU_DATA);

        let (data, working_counter) = self
            .pdu_loop
            .pdu_tx_readwrite(Command::Lrw { address }, value, &self.timeouts)
            .await?;

        if data.len() != value.len() {
            log::error!(
                "Data length {} does not match value length {}",
                data.len(),
                value.len()
            );
            return Err(Error::Pdu(PduError::Decode));
        }

        value.copy_from_slice(data);

        Ok((value, working_counter))
    }
}
