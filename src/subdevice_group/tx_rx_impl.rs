//! Methods implemented on `SubDeviceGroup` used to transfer process data.

use crate::{
    Command, MainDevice, RegisterAddress, SubDeviceGroup, TxRxResponse,
    al_control::AlControl,
    error::Error,
    fmt,
    pdu_loop::ReceivedPdu,
    subdevice_group::{CycleInfo, DC_PDU_SIZE, HasDc, HasPdi, MySyncUnsafeCell, push_state_checks},
};
use core::time::Duration;
use ethercrab_wire::EtherCrabWireRead;
use lock_api::{RawRwLock, RwLockWriteGuard};

impl<const MAX_SUBDEVICES: usize, const MAX_PDI: usize, R: RawRwLock, S, DC>
    SubDeviceGroup<MAX_SUBDEVICES, MAX_PDI, R, S, DC>
where
    S: HasPdi,
{
    /// Drive the SubDevice group's inputs and outputs.
    ///
    /// A `SubDeviceGroup` will not process any inputs or outputs unless this method is called
    /// periodically. It will send an `LRW` to update SubDevice outputs and read SubDevice inputs.
    ///
    /// This method returns a [`TxRxResponse`] containing the working counter and a list of all
    /// SubDevice states on success.
    ///
    /// # Errors
    ///
    /// This method will return with an error if the PDU could not be sent over the network, or the
    /// response times out.
    // Allow lint: `self.pdi.write()` and the frame await are linked so this is fine IMO. Hasn't
    // caused problems yet...
    #[allow(clippy::await_holding_lock)]
    pub async fn tx_rx<'sto>(
        &self,
        maindevice: &'sto MainDevice<'sto>,
    ) -> Result<TxRxResponse<MAX_SUBDEVICES>, Error> {
        fmt::trace!(
            "Group TX/RX, start address {:#010x}, data len {}, of which read bytes: {}",
            self.inner().pdi_start.start_address,
            self.pdi_len,
            self.read_pdi_len
        );

        let mut pdi_lock = self.pdi.write();

        let mut total_bytes_sent = 0;
        let mut lrw_wkc_sum = 0;

        let mut subdevices = self.inner().subdevices.iter();
        let mut total_checks = 0;
        let mut subdevice_states = heapless::Vec::<_, MAX_SUBDEVICES>::new();

        loop {
            let chunk_len = self.pdi_len.saturating_sub(total_bytes_sent);

            if chunk_len == 0 && total_checks >= self.len() {
                break;
            }

            let chunk_start = total_bytes_sent.min(self.pdi_len);
            let chunk = pdi_lock.get_mut()[chunk_start..(chunk_start + chunk_len)].as_ref();

            let mut frame = maindevice.pdu_loop.alloc_frame()?;

            // Start offset in the EtherCAT address space
            let pushed_chunk = if !chunk.is_empty() {
                let start_addr = self.inner().pdi_start.start_address + total_bytes_sent as u32;

                frame.push_pdu_slice_rest(Command::lrw(start_addr).into(), chunk)?
            } else {
                None
            };

            // If there's space left, push as many state checks as we can into the frame
            let (rest, num_checks_in_this_frame) = push_state_checks(subdevices, &mut frame)?;
            subdevices = rest;
            total_checks += num_checks_in_this_frame;

            if frame.is_empty() {
                break;
            }

            let frame = frame.mark_sendable(
                &maindevice.pdu_loop,
                maindevice.timeouts.pdu(),
                maindevice.config.retry_behaviour.retry_count(),
            );

            maindevice.pdu_loop.wake_sender();

            let received = frame.await?;

            let mut pdus = received.into_pdu_iter();

            // If we pushed a non-zero amount of PDI bytes, process the response
            if let Some((bytes_in_this_chunk, _pdu_handle)) = pushed_chunk {
                let wkc = self.process_received_pdi_chunk(
                    total_bytes_sent,
                    bytes_in_this_chunk,
                    &pdus.next().ok_or(Error::Internal)??,
                    &mut pdi_lock,
                )?;

                total_bytes_sent += bytes_in_this_chunk;
                lrw_wkc_sum += wkc;
            }

            // If there are any more PDUs, these are state checks
            for state_check_pdu in pdus {
                let state_check_pdu = state_check_pdu?;

                let state = AlControl::unpack_from_slice(&state_check_pdu)?;

                let _ = subdevice_states.push(state.state);
            }
        }

        Ok(TxRxResponse {
            working_counter: lrw_wkc_sum,
            subdevice_states,
            extra: (),
        })
    }

    /// Drive the SubDevice group's inputs and outputs and synchronise EtherCAT system time with
    /// `FRMW`.
    ///
    /// A `SubDeviceGroup` will not process any inputs or outputs unless this method is called
    /// periodically. It will send an `LRW` to update SubDevice outputs and read SubDevice inputs.
    ///
    /// This method returns a [`TxRxResponse`] struct, containing the working counter, group
    /// SubDevice statuses and the current EtherCAT system time in nanoseconds on success. If the
    /// PDI must be sent in multiple chunks, the returned working counter is the sum of all returned
    /// working counter values.
    ///
    /// # Errors
    ///
    /// This method will return with an error if the PDU could not be sent over the network, or the
    /// response times out.
    // Allow lint: `self.pdi.write()` and the frame await are linked so this is fine IMO. Hasn't
    // caused problems yet...
    #[allow(clippy::await_holding_lock)]
    pub async fn tx_rx_sync_system_time<'sto>(
        &self,
        maindevice: &'sto MainDevice<'sto>,
    ) -> Result<TxRxResponse<MAX_SUBDEVICES, Option<u64>>, Error> {
        let mut pdi_lock = self.pdi.write();

        fmt::trace!(
            "Group TX/RX with DC sync, start address {:#010x}, data len {}, of which read bytes: {}",
            self.inner().pdi_start.start_address,
            self.pdi_len,
            self.read_pdi_len
        );

        if let Some(dc_ref) = maindevice.dc_ref_address() {
            let mut total_bytes_sent = 0;
            let mut time = 0;
            let mut lrw_wkc_sum = 0;
            let mut time_read = false;

            let mut subdevices = self.inner().subdevices.iter();
            let mut total_checks = 0;
            let mut subdevice_states = heapless::Vec::<_, MAX_SUBDEVICES>::new();

            loop {
                let mut frame = maindevice.pdu_loop.alloc_frame()?;

                let dc_handle = if !time_read {
                    let dc_handle = frame.push_pdu(
                        Command::frmw(dc_ref, RegisterAddress::DcSystemTime.into()).into(),
                        0u64,
                        None,
                    )?;

                    // Just double checking
                    debug_assert_eq!(dc_handle.alloc_size, DC_PDU_SIZE);

                    Some(dc_handle)
                } else {
                    None
                };

                let chunk_start = total_bytes_sent.min(self.pdi_len);
                let chunk_len = self.pdi_len.saturating_sub(total_bytes_sent);
                let chunk = pdi_lock.get_mut()[chunk_start..(chunk_start + chunk_len)].as_ref();

                let pushed_chunk = if !chunk.is_empty() {
                    let start_addr = self.inner().pdi_start.start_address + total_bytes_sent as u32;

                    frame.push_pdu_slice_rest(Command::lrw(start_addr).into(), chunk)?
                } else {
                    None
                };

                if let Some((bytes_in_this_chunk, _)) = pushed_chunk {
                    fmt::trace!("Wrote {} byte chunk", bytes_in_this_chunk);
                }

                // If there's space left, push as many state checks as we can into the frame
                let (rest, num_checks_in_this_frame) = push_state_checks(subdevices, &mut frame)?;
                subdevices = rest;
                total_checks += num_checks_in_this_frame;

                if frame.is_empty() {
                    break Ok(TxRxResponse {
                        working_counter: lrw_wkc_sum,
                        subdevice_states,
                        extra: Some(time),
                    });
                }

                let frame = frame.mark_sendable(
                    &maindevice.pdu_loop,
                    maindevice.timeouts.pdu(),
                    maindevice.config.retry_behaviour.retry_count(),
                );

                maindevice.pdu_loop.wake_sender();

                let received = frame.await?;

                let mut pdus = received.into_pdu_iter();

                if dc_handle.is_some() {
                    let dc_pdu = pdus.next().ok_or(Error::Internal)?;

                    time =
                        dc_pdu.and_then(|rx| u64::unpack_from_slice(&rx).map_err(Error::from))?;

                    time_read = true;
                }

                // If we pushed a non-zero amount of PDI bytes, process the response
                if let Some((bytes_in_this_chunk, _pdu_handle)) = pushed_chunk {
                    let wkc = self.process_received_pdi_chunk(
                        total_bytes_sent,
                        bytes_in_this_chunk,
                        &pdus.next().ok_or(Error::Internal)??,
                        &mut pdi_lock,
                    )?;

                    total_bytes_sent += bytes_in_this_chunk;
                    lrw_wkc_sum += wkc;
                }

                // If there are any more PDUs, these are state checks
                for state_check_pdu in pdus {
                    let state_check_pdu = state_check_pdu?;

                    let state = AlControl::unpack_from_slice(&state_check_pdu)?;

                    let _ = subdevice_states.push(state.state);
                }

                // NOTE: Not using a while loop as we want to always send the DC sync PDU even if
                // the PDI is empty.
                if chunk_len == 0 && total_checks >= self.len() {
                    break Ok(TxRxResponse {
                        working_counter: lrw_wkc_sum,
                        subdevice_states,
                        extra: Some(time),
                    });
                }
            }
        } else {
            self.tx_rx(maindevice).await.map(|response| TxRxResponse {
                working_counter: response.working_counter,
                subdevice_states: response.subdevice_states,
                extra: None,
            })
        }
    }

    fn process_received_pdi_chunk(
        &self,
        total_bytes_sent: usize,
        bytes_in_this_chunk: usize,
        data: &ReceivedPdu<'_>,
        pdi_lock: &mut RwLockWriteGuard<'_, R, MySyncUnsafeCell<[u8; MAX_PDI]>>,
    ) -> Result<u16, Error> {
        let wkc = data.working_counter;

        let rx_range = total_bytes_sent.min(self.read_pdi_len)
            ..(total_bytes_sent + bytes_in_this_chunk).min(self.read_pdi_len);

        let inputs_chunk = &mut pdi_lock.get_mut()[rx_range];

        inputs_chunk.copy_from_slice(data.get(0..inputs_chunk.len()).ok_or(Error::Internal)?);

        Ok(wkc)
    }
}

// Methods for when the group has a PDI AND has Distributed Clocks configured
impl<const MAX_SUBDEVICES: usize, const MAX_PDI: usize, R: RawRwLock, S>
    SubDeviceGroup<MAX_SUBDEVICES, MAX_PDI, R, S, HasDc>
where
    S: HasPdi,
{
    /// Drive the SubDevice group's inputs and outputs, synchronise EtherCAT system time with
    /// `FRMW`, and return cycle timing and SubDevice state information.
    ///
    /// A `SubDeviceGroup` will not process any inputs or outputs unless this method is called
    /// periodically. It will send an `LRW` to update SubDevice outputs and read SubDevice inputs.
    ///
    /// This method returns a [`TxRxResponse`] struct, containing the working counter, a
    /// [`CycleInfo`] containing values that can be used to synchronise the MainDevice to the
    /// network SYNC0 event, and the state of all SubDevices in the group.
    ///
    /// # Errors
    ///
    /// This method will return with an error if the PDU could not be sent over the network, or the
    /// response times out.
    ///
    /// # Examples
    ///
    /// This example sends process data at 2.5ms offset into a 5ms cycle.
    ///
    /// ```rust,no_run
    /// # use ethercrab::{
    /// #     error::Error,
    /// #     subdevice_group::{CycleInfo, DcConfiguration, TxRxResponse},
    /// #     std::ethercat_now,
    /// #     MainDevice, MainDeviceConfig, PduStorage, Timeouts, DcSync,
    /// # };
    /// # use std::time::{Duration, Instant};
    /// # const MAX_SUBDEVICES: usize = 16;
    /// # const MAX_PDU_DATA: usize = PduStorage::element_size(1100);
    /// # const MAX_FRAMES: usize = 32;
    /// # const PDI_LEN: usize = 64;
    /// # static PDU_STORAGE: PduStorage<MAX_FRAMES, MAX_PDU_DATA> = PduStorage::new();
    /// # fn main() -> Result<(), Error> { smol::block_on(async {
    /// let (_tx, _rx, pdu_loop) = PDU_STORAGE.try_split().expect("can only split once");
    ///
    /// let maindevice = MainDevice::new(pdu_loop, Timeouts::default(), MainDeviceConfig::default());
    ///
    /// let cycle_time = Duration::from_millis(5);
    ///
    /// let mut group = maindevice
    ///     .init_single_group::<MAX_SUBDEVICES, PDI_LEN>(ethercat_now)
    ///     .await
    ///     .expect("Init");
    ///
    /// // This example enables SYNC0 for every detected SubDevice
    /// for mut subdevice in group.iter_mut(&maindevice) {
    ///     subdevice.set_dc_sync(DcSync::Sync0);
    /// }
    ///
    /// let mut group = group
    ///     .into_pre_op_pdi(&maindevice)
    ///     .await
    ///     .expect("PRE-OP -> PRE-OP with PDI")
    ///     .configure_dc_sync(
    ///         &maindevice,
    ///         DcConfiguration {
    ///             // Start SYNC0 100ms in the future
    ///             start_delay: Duration::from_millis(100),
    ///             // SYNC0 period should be the same as the process data loop in most cases
    ///             sync0_period: cycle_time,
    ///             // Send process data half way through cycle
    ///             sync0_shift: cycle_time / 2,
    ///         },
    ///     )
    ///     .await
    ///     .expect("DC configuration")
    ///     .request_into_op(&maindevice)
    ///     .await
    ///     .expect("PRE-OP -> SAFE-OP -> OP");
    ///
    /// // Wait for all SubDevices in the group to reach OP, whilst sending PDI to allow DC to start
    /// // correctly.
    /// loop {
    ///     let now = Instant::now();
    ///
    ///     let response @ TxRxResponse {
    ///         working_counter: _wkc,
    ///         extra: CycleInfo {
    ///             next_cycle_wait, ..
    ///         },
    ///         ..
    ///     } = group.tx_rx_dc(&maindevice).await.expect("TX/RX");
    ///
    ///     if response.all_op() {
    ///         break;
    ///     }
    ///
    ///     smol::Timer::at(now + next_cycle_wait).await;
    /// }
    ///
    /// // Main application process data cycle
    /// loop {
    ///     let now = Instant::now();
    ///
    ///     let TxRxResponse {
    ///         working_counter: _wkc,
    ///         extra: CycleInfo {
    ///             next_cycle_wait, ..
    ///         },
    ///         ..
    ///     } = group.tx_rx_dc(&maindevice).await.expect("TX/RX");
    ///
    ///     // Process data computations happen here
    ///
    ///     smol::Timer::at(now + next_cycle_wait).await;
    /// }
    /// # }) }
    /// ```
    // Allow lint: `self.pdi.write()` and the frame await are linked so this is fine IMO. Hasn't
    // caused problems yet...
    #[allow(clippy::await_holding_lock)]
    pub async fn tx_rx_dc<'sto>(
        &self,
        maindevice: &'sto MainDevice<'sto>,
    ) -> Result<TxRxResponse<MAX_SUBDEVICES, CycleInfo>, Error> {
        fmt::trace!(
            "Group TX/RX with DC sync, start address {:#010x}, data len {}, of which read bytes: {}",
            self.inner().pdi_start.start_address,
            self.pdi_len,
            self.read_pdi_len
        );

        let mut pdi_lock = self.pdi.write();

        let mut total_bytes_sent = 0;
        let mut time = 0;
        let mut lrw_wkc_sum = 0;
        let mut time_read = false;

        let mut subdevices = self.inner().subdevices.iter();
        let mut total_checks = 0;
        let mut subdevice_states = heapless::Vec::<_, MAX_SUBDEVICES>::new();

        loop {
            let mut frame = maindevice.pdu_loop.alloc_frame()?;

            let dc_handle = if !time_read {
                let dc_handle = frame.push_pdu(
                    Command::frmw(self.dc_conf.reference, RegisterAddress::DcSystemTime.into())
                        .into(),
                    0u64,
                    None,
                )?;

                // Just double checking
                debug_assert_eq!(dc_handle.alloc_size, DC_PDU_SIZE);

                Some(dc_handle)
            } else {
                None
            };

            let chunk_start = total_bytes_sent.min(self.pdi_len);
            let chunk_len = self.pdi_len.saturating_sub(total_bytes_sent);
            let chunk = pdi_lock.get_mut()[chunk_start..(chunk_start + chunk_len)].as_ref();

            let pushed_chunk = if !chunk.is_empty() {
                let start_addr = self.inner().pdi_start.start_address + total_bytes_sent as u32;

                frame.push_pdu_slice_rest(Command::lrw(start_addr).into(), chunk)?
            } else {
                None
            };

            // If there's space left, push as many state checks as we can into the frame
            let (rest, num_checks_in_this_frame) = push_state_checks(subdevices, &mut frame)?;
            subdevices = rest;
            total_checks += num_checks_in_this_frame;

            if frame.is_empty() {
                break;
            }

            let frame = frame.mark_sendable(
                &maindevice.pdu_loop,
                maindevice.timeouts.pdu(),
                maindevice.config.retry_behaviour.retry_count(),
            );

            maindevice.pdu_loop.wake_sender();

            let received = frame.await?;

            let mut pdus = received.into_pdu_iter();

            if dc_handle.is_some() {
                let dc_pdu = pdus.next().ok_or(Error::Internal)?;

                time = dc_pdu.and_then(|rx| u64::unpack_from_slice(&rx).map_err(Error::from))?;

                time_read = true;
            }

            // If we pushed a non-zero amount of PDI bytes, process the response
            if let Some((bytes_in_this_chunk, _pdu_handle)) = pushed_chunk {
                let wkc = self.process_received_pdi_chunk(
                    total_bytes_sent,
                    bytes_in_this_chunk,
                    &pdus.next().ok_or(Error::Internal)??,
                    &mut pdi_lock,
                )?;

                total_bytes_sent += bytes_in_this_chunk;
                lrw_wkc_sum += wkc;
            }

            // If there are any more PDUs, these are state checks
            for state_check_pdu in pdus {
                let state_check_pdu = state_check_pdu?;

                let state = AlControl::unpack_from_slice(&state_check_pdu)?;

                let _ = subdevice_states.push(state.state);
            }

            // NOTE: Not using a while loop as we want to always send the DC sync PDU even if the
            // PDI is empty.
            // This condition will exit the loop if the whole PDI has been sent as well as all
            // SubDevice status check PDUs.
            if chunk_len == 0 && total_checks >= self.len() {
                break;
            }
        }

        // Nanoseconds from the start of the cycle. This works because the first SYNC0 pulse
        // time is rounded to a whole number of `sync0_period`-length cycles.
        let cycle_start_offset = time % self.dc_conf.sync0_period;

        let time_to_next_iter =
            (self.dc_conf.sync0_period - cycle_start_offset) + self.dc_conf.sync0_shift;

        Ok(TxRxResponse {
            working_counter: lrw_wkc_sum,
            subdevice_states,
            extra: CycleInfo {
                dc_system_time: time,
                cycle_start_offset: Duration::from_nanos(cycle_start_offset),
                next_cycle_wait: Duration::from_nanos(time_to_next_iter),
            },
        })
    }
}
