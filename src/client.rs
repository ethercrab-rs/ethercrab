use crate::{
    al_control::AlControl,
    command::Command,
    dl_status::DlStatus,
    error::{Error, Item, PduError},
    pdi::PdiOffset,
    pdu_data::{PduData, PduRead},
    pdu_loop::{CheckWorkingCounter, PduLoop, PduResponse},
    register::{PortDescriptors, RegisterAddress, SupportFlags},
    slave::{slave_client::SlaveClient, Slave, Topology},
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
    // TODO: Find a way to retrieve a slave by index from the SlaveGroupContainer instead of
    // requiring a temporary array of slaves. Currently blocked by weird lifetime rubbish on
    // `SlaveGroupContainer`.
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

        // Latch receive times into all ports of all slaves.
        self.bwr(RegisterAddress::DcTimePort0, 0u32)
            .await
            .expect("Broadcast time")
            .wkc(num_slaves as u16, "Broadcast time")
            .unwrap();

        // let ethercat_offset = Utc.ymd(2000, 01, 01).and_hms(0, 0, 0);

        // let now_nanos =
        //     chrono::Utc::now().timestamp_nanos() - dbg!(ethercat_offset.timestamp_nanos());
        // TODO: Allow passing in of an initial value
        let now_nanos = 0;

        let mut delay_accum = 0;

        for i in 0..slaves.len() {
            let (parents, rest) = slaves.split_at_mut(i);
            let mut slave = rest.first_mut().ok_or(Error::Internal)?;

            {
                // Walk back up EtherCAT chain and find parent of this slave
                let mut parents_it = parents.iter().rev();

                // TODO: Check topology with two EK1100s, one daisychained off the other. I have a
                // feeling that won't work properly.
                while let Some(parent) = parents_it.next() {
                    // Previous parent in the chain is a leaf node in the tree, so we need to
                    // continue iterating to find the common parent, i.e. the split point
                    if parent.ports.topology() == Topology::LineEnd {
                        let split_point = parents_it
                            .find(|slave| slave.ports.topology() == Topology::Fork)
                            .ok_or_else(|| {
                                log::error!(
                                    "Did not find parent for slave {}",
                                    slave.configured_address
                                );

                                Error::Topology
                            })?;

                        slave.parent_index = Some(split_point.index);
                    } else {
                        slave.parent_index = Some(parent.index);

                        break;
                    }
                }
            }

            log::info!("Slave {:#06x} {}", slave.configured_address, slave.name);

            let sl = SlaveClient::new(self, slave.configured_address);

            let [time_p0, time_p1, time_p2, time_p3] = sl
                .read::<[u32; 4]>(RegisterAddress::DcTimePort0, "Port receive times")
                .await?;

            slave.ports.0[0].dc_receive_time = time_p0;
            slave.ports.0[1].dc_receive_time = time_p1;
            slave.ports.0[2].dc_receive_time = time_p2;
            slave.ports.0[3].dc_receive_time = time_p3;

            let d01 = time_p1 - time_p0;
            let d12 = time_p2 - time_p1;
            let d32 = time_p3 - time_p2;

            let loop_propagation_time = slave.ports.propagation_time();
            let child_delay = slave.ports.child_delay().unwrap_or(0);

            log::info!("--> Times {time_p0} ({d01}) {time_p1} ({d12}) {time_p2} ({d32}) {time_p3}");
            log::info!(
                "--> Propagation time {loop_propagation_time:?} ns, child delay {child_delay} ns"
            );

            if let Some(parent_idx) = slave.parent_index {
                let parent = parents
                    .iter_mut()
                    .find(|parent| parent.index == parent_idx)
                    .unwrap();

                let assigned_port_idx = parent
                    .ports
                    .assign_next_downstream_port(slave.index)
                    .expect("No free ports. Logic error.");

                let parent_port = parent.ports.0[assigned_port_idx];
                let prev_parent_port = parent.ports.prev_open_port(&parent_port).unwrap();
                let parent_time = parent_port.dc_receive_time - prev_parent_port.dc_receive_time;

                let entry_port = slave.ports.entry_port().unwrap();
                let prev_port = slave.ports.prev_open_port(&entry_port).unwrap();
                let my_time = prev_port.dc_receive_time - entry_port.dc_receive_time;

                // The delay between the previous slave and this one
                let delay = (parent_time - my_time) / 2;

                delay_accum += delay;

                // If parent has children but we're not one of them, add the children's delay to
                // this slave's offset.
                if let Some(child_delay) = parent
                    .ports
                    .child_delay()
                    .filter(|_| parent.ports.is_last_port(&parent_port))
                {
                    log::info!("--> Child delay of parent {}", child_delay);

                    delay_accum += child_delay / 2;
                }

                slave.propagation_delay = delay_accum;

                log::info!(
                    "--> Parent time {} ns, my time {} ns, delay {} ns (Δ {} ns)",
                    parent_time,
                    my_time,
                    delay_accum,
                    delay
                );
            }

            if !slave.flags.has_64bit_dc {
                // TODO?
                log::warn!("--> Slave uses seconds instead of ns?");
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
