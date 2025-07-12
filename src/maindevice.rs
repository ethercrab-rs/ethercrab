use crate::{
    BASE_SUBDEVICE_ADDRESS, MainDeviceConfig, SubDeviceGroup, Timeouts,
    al_control::AlControl,
    al_status_code::AlStatusCode,
    command::Command,
    dc,
    eeprom::types::SyncManager,
    error::{Error, Item},
    fmmu::Fmmu,
    fmt,
    pdi::PdiOffset,
    pdu_loop::{PduLoop, ReceivedPdu},
    register::RegisterAddress,
    subdevice::SubDevice,
    subdevice_group::{self, SubDeviceGroupHandle},
    subdevice_state::SubDeviceState,
    timer_factory::IntoTimeout,
};
use core::{
    cell::UnsafeCell,
    mem::size_of,
    sync::atomic::{AtomicU16, Ordering},
};
use ethercrab_wire::{EtherCrabWireSized, EtherCrabWireWrite};
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

unsafe impl Sync for MainDevice<'_> {}

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
    async fn blank_memory<const LEN: usize>(&self, start: impl Into<u16>) -> Result<(), Error> {
        let start = start.into();

        self.pdu_loop
            .pdu_broadcast_zeros(
                start,
                LEN as u16,
                self.timeouts.pdu,
                self.config.retry_behaviour.retry_count(),
            )
            .await
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

        // Clear FMMUs - see ETG1000.4 Table 57
        // Some devices aren't able to blank the entire region so we loop through all offsets.
        for fmmu_idx in 0..16 {
            self.blank_memory::<{ Fmmu::PACKED_LEN }>(RegisterAddress::fmmu(fmmu_idx))
                .await?;
        }

        // Clear SMs - see ETG1000.4 Table 59
        // Some devices aren't able to blank the entire region so we loop through all offsets.
        for sm_idx in 0..16 {
            self.blank_memory::<{ SyncManager::PACKED_LEN }>(RegisterAddress::sync_manager(sm_idx))
                .await?;
        }

        // Set DC control back to EtherCAT
        self.blank_memory::<{ size_of::<u8>() }>(RegisterAddress::DcCyclicUnitControl)
            .await?;
        self.blank_memory::<{ size_of::<u64>() }>(RegisterAddress::DcSystemTime)
            .await?;
        self.blank_memory::<{ size_of::<u64>() }>(RegisterAddress::DcSystemTimeOffset)
            .await?;
        self.blank_memory::<{ size_of::<u32>() }>(RegisterAddress::DcSystemTimeTransmissionDelay)
            .await?;
        self.blank_memory::<{ size_of::<u32>() }>(RegisterAddress::DcSystemTimeDifference)
            .await?;
        self.blank_memory::<{ size_of::<u8>() }>(RegisterAddress::DcSyncActive)
            .await?;
        self.blank_memory::<{ size_of::<u32>() }>(RegisterAddress::DcSyncStartTime)
            .await?;
        self.blank_memory::<{ size_of::<u32>() }>(RegisterAddress::DcSync0CycleTime)
            .await?;
        self.blank_memory::<{ size_of::<u32>() }>(RegisterAddress::DcSync1CycleTime)
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
            fmt::warn!(
                "No SubDevices were discovered. Check NIC device, connections and PDU response timeouts"
            );

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
        }

        // Now perform initial configuration for each subdevice. This is done in a separate loop
        // after all configured addresses are set to deal with the case where a powered on SD with a
        // set address is added to the network before init. In this case, two SDs could have the
        // same address which wouldn't have been reset yet when we're half way through a single
        // configuration loop.
        for subdevice_idx in 0..num_subdevices {
            let configured_address = BASE_SUBDEVICE_ADDRESS.wrapping_add(subdevice_idx);

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
                    .insert(usize::from(group.id()), UnsafeCell::new(group))
                    .map_err(|_| Error::Capacity(Item::Group))?;
            }

            let mut offset = PdiOffset::default();

            for (id, group) in group_map.into_iter() {
                let group = unsafe { *group.get() };

                offset = group.as_ref().into_pre_op(offset, self).await?;

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

        if addr > 0 { Some(addr) } else { None }
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

    #[allow(unused)]
    pub(crate) const fn max_frame_data(&self) -> usize {
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

    /// Release the [`PduLoop`] storage **without** resetting it.
    ///
    /// To reset the released `PduLoop`, call [`PduLoop::reset`]. This method does not release the
    /// network TX/RX handles created by e.g.
    /// [`PduStorage::try_split`](crate::PduStorage::try_split) to allow a new `MainDevice` to be
    /// created while reusing an existing network interface. To release the TX and RX handles as
    /// well, call [`release_all`](MainDevice::release_all).
    ///
    /// The application should ensure that no EtherCAT data is in flight when this method is called,
    /// i.e. all frames must have either returned to the MainDevice or timed out. If a frame is
    /// received after this method has been called, the [`PduRx`](crate::PduRx) instance handling
    /// that frame will most likely produce an error as the underlying storage for that frame has
    /// been freed.
    ///
    /// # Safety
    ///
    /// Any groups configured using the previous `MainDevice` instance **must not** be used again
    /// with any `MainDevice`s created with the `PduLoop` returned by this method. The group state
    /// (PDI addresses, offsets, sizes, etc) are only valid with the `MainDevice` the group was
    /// initialised with.
    pub unsafe fn release(mut self) -> PduLoop<'sto> {
        // Clear out any in-use frames.
        self.pdu_loop.reset();

        self.pdu_loop
    }

    /// Release the [`PduLoop`] storage and signal the TX/RX handles to release their resources.
    ///
    /// This method is useful to close down a TX/RX loop and the network interface associated with
    /// it.
    ///
    /// To reuse the TX/RX loop and only free the `PduLoop` for reuse in another `MainDevice`
    /// instance, call [`release`](MainDevice::release).
    ///
    /// The application should ensure that no EtherCAT data is in flight when this method is called,
    /// i.e. all frames must have either returned to the MainDevice or timed out. If a frame is
    /// received after this method has been called, the [`PduRx`](crate::PduRx) instance handling
    /// that frame will most likely produce an error as the underlying storage for that frame has
    /// been freed.
    ///
    /// # Safety
    ///
    /// Any groups configured using the previous `MainDevice` instance **must not** be used again
    /// with any `MainDevice`s created with the `PduLoop` returned by this method. The group state
    /// (PDI addresses, offsets, sizes, etc) are only valid with the `MainDevice` the group was
    /// initialised with.
    pub unsafe fn release_all(mut self) -> PduLoop<'sto> {
        self.pdu_loop.reset_all();

        // Wake the TX/RX loop up so it can check for the stop flag
        self.pdu_loop.wake_sender();

        self.pdu_loop
    }
}
