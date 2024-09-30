use crate::{
    al_control::AlControl,
    al_status_code::AlStatusCode,
    command::Command,
    dc,
    error::{Error, Item},
    fmt,
    pdi::PdiOffset,
    pdu_loop::{PduLoop, ReceivedPdu},
    register::RegisterAddress,
    subdevice::SubDevice,
    subdevice_group::{self, SubDeviceGroupHandle},
    subdevice_state::SubDeviceState,
    timer_factory::IntoTimeout,
    MainDeviceConfig, SubDeviceGroup, Timeouts, BASE_SUBDEVICE_ADDRESS,
};
use core::{
    ops::Range,
    sync::atomic::{AtomicU16, Ordering},
};
use ethercrab_wire::EtherCrabWireWrite;
use heapless::FnvIndexMap;

/// The main EtherCAT controller.
///
/// The `MainDevice` is passed by reference to [`SubDeviceGroup`]s to drive their TX/RX methods. It
/// also provides direct access to EtherCAT PDUs like `BRD`, `LRW`, etc.
#[doc(alias = "Client")]
#[doc(alias = "Master")]
#[derive(Debug)]
pub struct MainDevice<'sto> {
    pub(crate) pdu_loop: PduLoop<'sto>,
    /// The total number of discovered subdevices.
    ///
    /// Using an `AtomicU16` here only to satisfy `Sync` requirements, but it's only ever written to
    /// once so its safety is largely unused.
    num_subdevices: AtomicU16,
    /// DC reference clock.
    ///
    /// If no DC subdevices are found, this will be `0`.
    dc_reference_configured_address: AtomicU16,
    pub(crate) timeouts: Timeouts,
    pub(crate) config: MainDeviceConfig,
}

unsafe impl<'sto> Sync for MainDevice<'sto> {}

impl<'sto> MainDevice<'sto> {
    /// Create a new EtherCrab MainDevice.
    pub const fn new(
        pdu_loop: PduLoop<'sto>,
        timeouts: Timeouts,
        config: MainDeviceConfig,
    ) -> Self {
        Self {
            pdu_loop,
            num_subdevices: AtomicU16::new(0),
            dc_reference_configured_address: AtomicU16::new(0),
            timeouts,
            config,
        }
    }

    /// Write zeroes to every SubDevice's memory in chunks.
    async fn blank_memory(&self, start: impl Into<u16>, len: u16) -> Result<(), Error> {
        let step = self.pdu_loop.max_frame_data();

        for chunk in blank_mem_iter(start.into(), len, step) {
            let chunk_len = chunk.end - chunk.start;

            self.pdu_loop
                .pdu_broadcast_zeros(
                    chunk.start,
                    chunk_len,
                    self.timeouts.pdu,
                    self.config.retry_behaviour.retry_count(),
                )
                .await?;
        }

        Ok(())
    }

    // FIXME: When adding a powered on SubDevice to the network, something breaks. Maybe need to reset
    // the configured address? But this broke other stuff so idk...
    async fn reset_subdevices(&self) -> Result<(), Error> {
        fmt::debug!("Beginning reset");

        // Reset SubDevices to init
        Command::bwr(RegisterAddress::AlControl.into())
            .ignore_wkc()
            .send(self, AlControl::reset())
            .await?;

        // Clear FMMUs. FMMU memory section is 0xff (255) bytes long - see ETG1000.4 Table 57
        self.blank_memory(RegisterAddress::Fmmu0, 0xff).await?;

        // Clear SMs. SM memory section is 0x7f bytes long - see ETG1000.4 Table 59
        self.blank_memory(RegisterAddress::Sm0, 0x7f).await?;

        // Set DC control back to EtherCAT
        self.blank_memory(
            RegisterAddress::DcCyclicUnitControl,
            core::mem::size_of::<u8>() as u16,
        )
        .await?;
        self.blank_memory(
            RegisterAddress::DcSystemTime,
            core::mem::size_of::<u64>() as u16,
        )
        .await?;
        self.blank_memory(
            RegisterAddress::DcSystemTimeOffset,
            core::mem::size_of::<u64>() as u16,
        )
        .await?;
        self.blank_memory(
            RegisterAddress::DcSystemTimeTransmissionDelay,
            core::mem::size_of::<u32>() as u16,
        )
        .await?;
        self.blank_memory(
            RegisterAddress::DcSystemTimeDifference,
            core::mem::size_of::<u32>() as u16,
        )
        .await?;
        self.blank_memory(
            RegisterAddress::DcSyncActive,
            core::mem::size_of::<u8>() as u16,
        )
        .await?;
        self.blank_memory(
            RegisterAddress::DcSyncStartTime,
            core::mem::size_of::<u32>() as u16,
        )
        .await?;
        self.blank_memory(
            RegisterAddress::DcSync0CycleTime,
            core::mem::size_of::<u32>() as u16,
        )
        .await?;
        self.blank_memory(
            RegisterAddress::DcSync1CycleTime,
            core::mem::size_of::<u32>() as u16,
        )
        .await?;

        // ETG1020 Section 22.2.4 defines these initial parameters. The data types are defined in
        // ETG1000.4 Table 60 – Distributed clock local time parameter, helpfully named "Control
        // Loop Parameter 1" to 3.
        //
        // According to ETG1020, we'll use the mode where the DC reference clock is adjusted to the
        // master clock.
        Command::bwr(RegisterAddress::DcControlLoopParam3.into())
            .ignore_wkc()
            .send(self, 0x0c00u16)
            .await?;
        // Must be after param 3 so DC control unit is reset
        Command::bwr(RegisterAddress::DcControlLoopParam1.into())
            .ignore_wkc()
            .send(self, 0x1000u16)
            .await?;

        fmt::debug!("--> Reset complete");

        Ok(())
    }

    /// Detect SubDevices, set their configured station addresses, assign to groups, configure
    /// SubDevices from EEPROM.
    ///
    /// This method will request and wait for all SubDevices to be in `PRE-OP` before returning.
    ///
    /// To transition groups into different states, see [`SubDeviceGroup::into_safe_op`] or
    /// [`SubDeviceGroup::into_op`].
    ///
    /// The `group_filter` closure should return a [`&dyn
    /// SubDeviceGroupHandle`](crate::subdevice_group::SubDeviceGroupHandle) to add the SubDevice
    /// to. All SubDevices must be assigned to a group even if they are unused.
    ///
    /// If a SubDevice cannot or should not be added to a group for some reason (e.g. an
    /// unrecognised SubDevice was detected on the network), an
    /// [`Err(Error::UnknownSubDevice)`](Error::UnknownSubDevice) should be returned.
    ///
    /// `MAX_SUBDEVICES` must be a power of 2 greater than 1.
    ///
    /// Note that the sum of the PDI data length for all [`SubDeviceGroup`]s must not exceed the
    /// value of `MAX_PDU_DATA`.
    ///
    /// # Examples
    ///
    /// ## Multiple groups
    ///
    /// This example groups SubDevices into two different groups.
    ///
    /// ```rust,no_run
    /// use ethercrab::{
    ///     error::Error, std::{ethercat_now, tx_rx_task}, MainDevice, MainDeviceConfig, PduStorage,
    ///     SubDeviceGroup, Timeouts, subdevice_group
    /// };
    ///
    /// const MAX_SUBDEVICES: usize = 2;
    /// const MAX_PDU_DATA: usize = PduStorage::element_size(1100);
    /// const MAX_FRAMES: usize = 16;
    ///
    /// static PDU_STORAGE: PduStorage<MAX_FRAMES, MAX_PDU_DATA> = PduStorage::new();
    ///
    /// /// A custom struct containing two groups to assign SubDevices into.
    /// #[derive(Default)]
    /// struct Groups {
    ///     /// 2 SubDevices, totalling 1 byte of PDI.
    ///     group_1: SubDeviceGroup<2, 1>,
    ///     /// 1 SubDevice, totalling 4 bytes of PDI
    ///     group_2: SubDeviceGroup<1, 4>,
    /// }
    ///
    /// let (_tx, _rx, pdu_loop) = PDU_STORAGE.try_split().expect("can only split once");
    ///
    /// let maindevice = MainDevice::new(pdu_loop, Timeouts::default(), MainDeviceConfig::default());
    ///
    /// # async {
    /// let groups = maindevice
    ///     .init::<MAX_SUBDEVICES, _>(ethercat_now, |groups: &Groups, subdevice| {
    ///         match subdevice.name() {
    ///             "COUPLER" | "IO69420" => Ok(&groups.group_1),
    ///             "COOLSERVO" => Ok(&groups.group_2),
    ///             _ => Err(Error::UnknownSubDevice),
    ///         }
    ///     },)
    ///     .await
    ///     .expect("Init");
    /// # };
    /// ```
    pub async fn init<const MAX_SUBDEVICES: usize, G>(
        &self,
        now: impl Fn() -> u64 + Copy,
        mut group_filter: impl for<'g> FnMut(
            &'g G,
            &SubDevice,
        ) -> Result<&'g dyn SubDeviceGroupHandle, Error>,
    ) -> Result<G, Error>
    where
        G: Default,
    {
        let groups = G::default();

        // Each SubDevice increments working counter, so we can use it as a total count of
        // SubDevices
        let num_subdevices = self.count_subdevices().await?;

        fmt::debug!("Discovered {} SubDevices", num_subdevices);

        if num_subdevices == 0 {
            fmt::warn!("No SubDevices were discovered. Check NIC device, connections and PDU response timeouts");

            return Ok(groups);
        }

        self.reset_subdevices().await?;

        // This is the only place we store the number of SubDevices, so the ordering can be
        // pretty much anything.
        self.num_subdevices.store(num_subdevices, Ordering::Relaxed);

        let mut subdevices = heapless::Deque::<SubDevice, MAX_SUBDEVICES>::new();

        // Set configured address for all discovered SubDevices
        for subdevice_idx in 0..num_subdevices {
            let configured_address = BASE_SUBDEVICE_ADDRESS.wrapping_add(subdevice_idx);

            Command::apwr(
                subdevice_idx,
                RegisterAddress::ConfiguredStationAddress.into(),
            )
            .send(self, configured_address)
            .await?;

            let subdevice = SubDevice::new(self, subdevice_idx, configured_address).await?;

            subdevices
                .push_back(subdevice)
                .map_err(|_| Error::Capacity(Item::SubDevice))?;
        }

        fmt::debug!("Configuring topology/distributed clocks");

        // Configure distributed clock offsets/propagation delays, perform static drift
        // compensation. We need the SubDevices in a single list so we can read the topology.
        let dc_master = dc::configure_dc(self, subdevices.as_mut_slices().0, now).await?;

        // If there are SubDevices that support distributed clocks, run static drift compensation
        if let Some(dc_master) = dc_master {
            self.dc_reference_configured_address
                .store(dc_master.configured_address(), Ordering::Relaxed);

            dc::run_dc_static_sync(self, dc_master, self.config.dc_static_sync_iterations).await?;
        }

        // This block is to reduce the lifetime of the groups map references
        {
            // A unique list of groups so we can iterate over them and assign consecutive PDIs to each
            // one.
            let mut group_map = FnvIndexMap::<_, _, MAX_SUBDEVICES>::new();

            while let Some(subdevice) = subdevices.pop_front() {
                let group = group_filter(&groups, &subdevice)?;

                // SAFETY: This mutates the internal SubDevice list, so a reference to `group` may not be
                // held over this line.
                unsafe { group.push(subdevice)? };

                group_map
                    .insert(usize::from(group.id()), group.as_ref())
                    .map_err(|_| Error::Capacity(Item::Group))?;
            }

            let mut offset = PdiOffset::default();

            for (id, group) in group_map.iter_mut() {
                offset = group.into_pre_op(offset, self).await?;

                fmt::debug!("After group ID {} offset: {:?}", id, offset);
            }

            fmt::debug!("Total PDI {} bytes", offset.start_address);
        }

        // Check that all SubDevices reached PRE-OP
        self.wait_for_state(SubDeviceState::PreOp).await?;

        Ok(groups)
    }

    /// A convenience method to allow the quicker creation of a single group containing all
    /// discovered SubDevices.
    ///
    /// This method will request and wait for all SubDevices to be in `PRE-OP` before returning.
    ///
    /// To transition groups into different states, see [`SubDeviceGroup::into_safe_op`] or
    /// [`SubDeviceGroup::into_op`].
    ///
    /// For multiple groups, see [`MainDevice::init`].
    ///
    /// # Examples
    ///
    /// ## Create a single SubDevice group with no `PREOP -> SAFEOP` configuration
    ///
    /// ```rust,no_run
    /// use ethercrab::{
    ///     error::Error, MainDevice, MainDeviceConfig, PduStorage, Timeouts, std::ethercat_now
    /// };
    ///
    /// const MAX_SUBDEVICES: usize = 2;
    /// const MAX_PDU_DATA: usize = PduStorage::element_size(1100);
    /// const MAX_FRAMES: usize = 16;
    /// const MAX_PDI: usize = 8;
    ///
    /// static PDU_STORAGE: PduStorage<MAX_FRAMES, MAX_PDU_DATA> = PduStorage::new();
    ///
    /// let (_tx, _rx, pdu_loop) = PDU_STORAGE.try_split().expect("can only split once");
    ///
    /// let maindevice = MainDevice::new(pdu_loop, Timeouts::default(), MainDeviceConfig::default());
    ///
    /// # async {
    /// let group = maindevice
    ///     .init_single_group::<MAX_SUBDEVICES, MAX_PDI>(ethercat_now)
    ///     .await
    ///     .expect("Init");
    /// # };
    /// ```
    ///
    /// ## Create a single SubDevice group with `PREOP -> SAFEOP` configuration of SDOs
    ///
    /// ```rust,no_run
    /// use ethercrab::{
    ///     error::Error, MainDevice, MainDeviceConfig, PduStorage, Timeouts, std::ethercat_now
    /// };
    ///
    /// const MAX_SUBDEVICES: usize = 2;
    /// const MAX_PDU_DATA: usize = PduStorage::element_size(1100);
    /// const MAX_FRAMES: usize = 16;
    /// const MAX_PDI: usize = 8;
    ///
    /// static PDU_STORAGE: PduStorage<MAX_FRAMES, MAX_PDU_DATA> = PduStorage::new();
    ///
    /// let (_tx, _rx, pdu_loop) = PDU_STORAGE.try_split().expect("can only split once");
    ///
    /// let maindevice = MainDevice::new(pdu_loop, Timeouts::default(), MainDeviceConfig::default());
    ///
    /// # async {
    /// let mut group = maindevice
    ///     .init_single_group::<MAX_SUBDEVICES, MAX_PDI>(ethercat_now)
    ///     .await
    ///     .expect("Init");
    ///
    /// for subdevice in group.iter(&maindevice) {
    ///     if subdevice.name() == "EL3004" {
    ///         log::info!("Found EL3004. Configuring...");
    ///
    ///         subdevice.sdo_write(0x1c12, 0, 0u8).await?;
    ///         subdevice.sdo_write(0x1c13, 0, 0u8).await?;
    ///
    ///         subdevice.sdo_write(0x1c13, 1, 0x1a00u16).await?;
    ///         subdevice.sdo_write(0x1c13, 2, 0x1a02u16).await?;
    ///         subdevice.sdo_write(0x1c13, 3, 0x1a04u16).await?;
    ///         subdevice.sdo_write(0x1c13, 4, 0x1a06u16).await?;
    ///         subdevice.sdo_write(0x1c13, 0, 4u8).await?;
    ///     }
    /// }
    ///
    /// let mut group = group.into_safe_op(&maindevice).await.expect("PRE-OP -> SAFE-OP");
    /// # Ok::<(), ethercrab::error::Error>(())
    /// # };
    /// ```
    pub async fn init_single_group<const MAX_SUBDEVICES: usize, const MAX_PDI: usize>(
        &self,
        now: impl Fn() -> u64 + Copy,
    ) -> Result<SubDeviceGroup<MAX_SUBDEVICES, MAX_PDI, subdevice_group::PreOp>, Error> {
        self.init::<MAX_SUBDEVICES, _>(now, |group, _subdevice| Ok(group))
            .await
    }

    /// Count the number of SubDevices on the network.
    async fn count_subdevices(&self) -> Result<u16, Error> {
        Command::brd(RegisterAddress::Type.into())
            .receive_wkc::<u8>(self)
            .await
    }

    /// Get the number of discovered SubDevices in the EtherCAT network.
    ///
    /// As [`init`](crate::MainDevice::init) runs SubDevice autodetection, it must be called before this
    /// method to get an accurate count.
    pub fn num_subdevices(&self) -> usize {
        usize::from(self.num_subdevices.load(Ordering::Relaxed))
    }

    /// Get the configured address of the designated DC reference subdevice.
    pub(crate) fn dc_ref_address(&self) -> Option<u16> {
        let addr = self.dc_reference_configured_address.load(Ordering::Relaxed);

        if addr > 0 {
            Some(addr)
        } else {
            None
        }
    }

    /// Wait for all SubDevices on the network to reach a given state.
    pub async fn wait_for_state(&self, desired_state: SubDeviceState) -> Result<(), Error> {
        let num_subdevices = self.num_subdevices.load(Ordering::Relaxed);

        async {
            loop {
                let status = Command::brd(RegisterAddress::AlStatus.into())
                    .with_wkc(num_subdevices)
                    .receive::<AlControl>(self)
                    .await?;

                fmt::trace!("Global AL status {:?}", status);

                if status.error {
                    fmt::error!(
                        "Error occurred transitioning all SubDevices to {:?}",
                        desired_state,
                    );

                    for subdevice_addr in BASE_SUBDEVICE_ADDRESS
                        ..(BASE_SUBDEVICE_ADDRESS + self.num_subdevices() as u16)
                    {
                        let status =
                            Command::fprd(subdevice_addr, RegisterAddress::AlStatusCode.into())
                                .ignore_wkc()
                                .receive::<AlStatusCode>(self)
                                .await
                                .unwrap_or(AlStatusCode::UnspecifiedError);

                        fmt::error!(
                            "--> SubDevice {:#06x} status code {}",
                            subdevice_addr,
                            status
                        );
                    }

                    return Err(Error::StateTransition);
                }

                if status.state == desired_state {
                    break Ok(());
                }

                self.timeouts.loop_tick().await;
            }
        }
        .timeout(self.timeouts.state_transition)
        .await
    }

    pub(crate) fn max_frame_data(&self) -> usize {
        self.pdu_loop.max_frame_data()
    }

    /// Send a single PDU in a frame.
    pub(crate) async fn single_pdu(
        &'sto self,
        command: Command,
        data: impl EtherCrabWireWrite,
        len_override: Option<u16>,
    ) -> Result<ReceivedPdu<'sto>, Error> {
        let mut frame = self.pdu_loop.alloc_frame()?;

        let handle = frame.push_pdu(command, data, len_override)?;

        let frame = frame.mark_sendable(
            &self.pdu_loop,
            self.timeouts.pdu,
            self.config.retry_behaviour.retry_count(),
        );

        self.pdu_loop.wake_sender();

        frame.await?.first_pdu(handle)
    }
}

fn blank_mem_iter(
    start: impl Into<u16>,
    mut len: u16,
    step: usize,
) -> impl Iterator<Item = Range<u16>> + Clone {
    let start: u16 = start.into();
    let mut range = (start..(start + len)).step_by(step.max(1));

    core::iter::from_fn(move || {
        if len == 0 || step == 0 {
            return None;
        }

        range.next().map(|chunk_start| {
            let chunk_len = (step as u16).min(len);

            len -= chunk_len;

            chunk_start..(chunk_start + chunk_len)
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg_attr(miri, ignore)]
    fn blank_mem_fuzz() {
        heckcheck::check(|(start, len, step): (u16, u16, u16)| {
            // For this test, anything that overflows we ignore. This is allowed to panic in prod
            // IMO.
            if u32::from(start) + u32::from(len) > u32::from(u16::MAX) {
                return Ok(());
            }

            // Ensure we add another iteration for a partial final step
            let iterations = len.checked_div(step).unwrap_or(0) + (len % step).min(1);

            let step = usize::from(step);

            let end = start + len;

            let it = blank_mem_iter(start, len, step);

            assert_eq!(it.clone().count(), usize::from(iterations));
            assert_eq!(
                it.last().map(|l| l.end),
                if iterations == 0 { None } else { Some(end) },
                "start {} end {} len {}",
                start,
                end,
                len
            );

            Ok(())
        });
    }
}
