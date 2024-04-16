//! An extremely crude example that runs two DC-enabeld LAN9252 at different cycle times to check
//! SYNC0 alignment.

use env_logger::Env;
use ethercrab::{
    error::Error,
    slave_group::{CycleInfo, DcConfiguration},
    std::{ethercat_now, tx_rx_task},
    Client, ClientConfig, DcSync, PduStorage, RegisterAddress, Timeouts,
};
use futures_lite::StreamExt;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

/// Maximum number of slaves that can be stored. This must be a power of 2 greater than 1.
const MAX_SLAVES: usize = 16;
const MAX_PDU_DATA: usize = PduStorage::element_size(1100);
const MAX_FRAMES: usize = 32;
const PDI_LEN: usize = 64;

static PDU_STORAGE: PduStorage<MAX_FRAMES, MAX_PDU_DATA> = PduStorage::new();

const TICK_INTERVAL: Duration = Duration::from_millis(5);

#[allow(unused)]
#[derive(Debug, ethercrab_wire::EtherCrabWireRead)]
#[wire(bytes = 2)]
pub struct SupportedModes {
    /// Bit 0 = 1: free run is supported.
    #[wire(bits = 1)]
    free_run: bool,
    /// Bit 1 = 1: Synchronous with SM 2 event is supported.
    #[wire(bits = 1)]
    sm2: bool,
    /// Bit 2-3 = 01: DC mode is supported.
    #[wire(bits = 2)]
    dc_supported: bool,
    /// Bit 4-5 = 10: Output shift with SYNC1 event (only DC mode).
    #[wire(bits = 2)]
    sync1: bool,
    /// Bit 14 = 1: dynamic times (measurement through writing of 0x1C32:08).
    #[wire(pre_skip = 8, bits = 1, post_skip = 1)]
    dynamic: bool,
}

fn main() -> Result<(), Error> {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    let interface = std::env::args()
        .nth(1)
        .expect("Provide network interface as first argument.");

    log::info!("Starting Distributed Clocks demo...");
    log::info!("Run with RUST_LOG=ethercrab=debug or =trace for debug information");

    let (tx, rx, pdu_loop) = PDU_STORAGE.try_split().expect("can only split once");

    let client = Arc::new(Client::new(
        pdu_loop,
        Timeouts::default(),
        ClientConfig::default(),
    ));

    let mut tick_interval = smol::Timer::interval(TICK_INTERVAL);

    smol::spawn(tx_rx_task(&interface, tx, rx).expect("spawn TX/RX task")).detach();

    #[cfg(target_os = "linux")]
    thread_priority::set_current_thread_priority(thread_priority::ThreadPriority::Crossplatform(
        thread_priority::ThreadPriorityValue::try_from(48u8).unwrap(),
    ))
    .expect("Main thread prio");

    smol::block_on(async {
        let mut group = client
            .init_single_group::<MAX_SLAVES, PDI_LEN>(ethercat_now)
            .await
            .expect("Init");

        for mut slave in group.iter(&client) {
            if slave.name() == "LAN9252-EVB-HBI" {
                // Sync mode 02 = SYNC0
                slave
                    .sdo_write(0x1c32, 1, 2u16)
                    .await
                    .expect("Set sync mode");

                // ETG1020 calc and copy time
                let cal_and_copy_time = slave
                    .sdo_read::<u16>(0x1c32, 6)
                    .await
                    .expect("Calc and copy time");

                // Delay time
                let delay_time = slave.sdo_read::<u16>(0x1c32, 9).await.expect("Delay time");

                log::info!(
                    "LAN9252 calc time {} ns, delay time {} ns",
                    cal_and_copy_time,
                    delay_time,
                );

                // Adding this seems to make the second LAN9252 converge much more quickly
                slave
                    .sdo_write(0x1c32, 0x0a, TICK_INTERVAL.as_nanos() as u32)
                    .await
                    .expect("Set cycle time");

                let sync_type = slave.sdo_read::<u16>(0x1c32, 1).await?;
                let cycle_time = slave.sdo_read::<u32>(0x1c32, 2).await?;
                let min_cycle_time = slave.sdo_read::<u32>(0x1c32, 5).await?;
                let supported_sync_modes = slave.sdo_read::<SupportedModes>(0x1c32, 4).await?;
                log::info!("--> Outputs sync mode {sync_type}, cycle time {cycle_time} ns (min {min_cycle_time} ns), supported modes {supported_sync_modes:?}");

                let sync_type = slave.sdo_read::<u16>(0x1c33, 1).await?;
                let cycle_time = slave.sdo_read::<u32>(0x1c33, 2).await?;
                let min_cycle_time = slave.sdo_read::<u32>(0x1c33, 5).await?;
                let supported_sync_modes = slave.sdo_read::<SupportedModes>(0x1c33, 4).await?;
                log::info!("--> Inputs sync mode {sync_type}, cycle time {cycle_time} ns (min {min_cycle_time} ns), supported modes {supported_sync_modes:?}");
            }

            // EL4102 requires SYNC0 AND SYNC1 to operate
            if slave.name() == "EL4102" {
                log::info!("Found EL4102");

                // Sync mode 02 = SYNC0
                slave
                    .sdo_write(0x1c32, 1, 2u16)
                    .await
                    .expect("Set sync mode");

                slave
                    .sdo_write(0x1c32, 0x02, TICK_INTERVAL.as_nanos() as u32)
                    .await
                    .expect("Set cycle time");

                // ETG1020 calc and copy time
                let cal_and_copy_time = slave
                    .sdo_read::<u16>(0x1c32, 6)
                    .await
                    .expect("Calc and copy time");

                // Delay time
                let delay_time = slave.sdo_read::<u16>(0x1c32, 9).await.expect("Delay time");

                log::info!(
                    "--> Calc time {} ns, delay time {} ns",
                    cal_and_copy_time,
                    delay_time,
                );

                let sync_type = slave.sdo_read::<u16>(0x1c32, 1).await?;
                let cycle_time = slave.sdo_read::<u32>(0x1c32, 2).await?;
                let shift_time = slave.sdo_read::<u32>(0x1c32, 3).await?;
                let min_cycle_time = slave.sdo_read::<u32>(0x1c32, 5).await?;
                let supported_sync_modes = slave.sdo_read::<SupportedModes>(0x1c32, 4).await?;
                // NOTE: For EL4102, SupportedModes.sync1 is false, but the ESI file specifies it,
                // and the 4102 won't go into OP without setting up SYNC1 with the correct offset.
                // Brilliant.
                log::info!("--> Outputs sync mode {sync_type}, cycle time {cycle_time} ns (min {min_cycle_time} ns), shift {shift_time} ns, supported modes {supported_sync_modes:?}");

                slave.set_dc_sync(DcSync::Sync01 {
                    // EL4102 ESI specifies SYNC1 with an offset of 100k ns
                    sync1_period: Duration::from_nanos(100_000),
                });
            } else {
                // Enable just SYNC0 for any other SubDevice
                slave.set_dc_sync(DcSync::Sync0);
            }
        }

        log::info!("Moving into PRE-OP with PDI");

        let mut group = group.into_pre_op_pdi(&client).await?;

        log::info!("Done. PDI available. Waiting for SubDevices to align");

        let mut now = Instant::now();
        let start = Instant::now();

        loop {
            group.tx_rx_sync_system_time(&client).await.expect("TX/RX");

            if now.elapsed() >= Duration::from_millis(25) {
                now = Instant::now();

                let mut max_deviation = 0;

                for s1 in group.iter(&client) {
                    let diff = match s1
                        .register_read::<u32>(RegisterAddress::DcSystemTimeDifference)
                        .await
                        // The returned value is NOT in two's compliment, rather the upper bit
                        // specifies whether the number in the remaining bits is odd or even, so we
                        // convert the value to `i32` using that logic here.
                        .map(|value| {
                            let flag = 0b1u32 << 31;

                            let less_than = value & flag > 0;

                            let value = value & !flag;

                            if less_than {
                                -(value as i32)
                            } else {
                                value as i32
                            }
                        }) {
                        Ok(diff) => diff,
                        Err(Error::WorkingCounter { .. }) => 0,
                        Err(e) => return Err(e),
                    };

                    max_deviation = max_deviation.max(diff.abs());
                }

                log::debug!("--> Max deviation {} ns", max_deviation);

                // Less than 100ns max deviation as an example threshold.
                // <https://github.com/OpenEtherCATsociety/SOEM/issues/487#issuecomment-786245585>
                // mentions less than 100us as a good enough value as well.
                if max_deviation < 1_000 {
                    log::info!("Clocks settled after {} ms", start.elapsed().as_millis());

                    break;
                }
            }

            tick_interval.next().await;
        }

        log::info!("Group alignment done");

        // SubDevice clocks are aligned. We can turn DC on now.
        let group = group
            .configure_dc_sync(
                &client,
                DcConfiguration {
                    // Start SYNC0 100ms in the future
                    start_delay: Duration::from_millis(100),
                    // SYNC0 period should be the same as the process data loop in most cases
                    sync0_period: TICK_INTERVAL,
                    // Send process data half way through cycle
                    sync0_shift: TICK_INTERVAL / 2,
                },
            )
            .await?;

        let group = group
            .into_safe_op(&client)
            .await
            .expect("PRE-OP -> SAFE-OP");

        log::info!("SAFE-OP");

        let term = Arc::new(AtomicBool::new(false));
        signal_hook::flag::register(signal_hook::consts::SIGINT, Arc::clone(&term))
            .expect("Register hook");

        // Request OP state without waiting for all SubDevices to reach it. Allows the immediate
        // start of the process data cycle, which is required when DC sync is used, otherwise
        // SubDevices never reach OP, most often timing out with a SyncManagerWatchdog error.
        let mut group = group.request_into_op(&client).await.expect("SAFE-OP -> OP");

        log::info!("OP requested");

        let op_request = Instant::now();

        while !group.all_op(&client).await? {
            let now = Instant::now();

            let (
                _wkc,
                CycleInfo {
                    next_cycle_wait, ..
                },
            ) = group.tx_rx_dc(&client).await.expect("TX/RX");

            smol::Timer::at(now + next_cycle_wait).await;
        }

        log::info!(
            "All SubDevices entered OP in {} us",
            op_request.elapsed().as_micros()
        );

        loop {
            let now = Instant::now();

            let (
                _wkc,
                CycleInfo {
                    next_cycle_wait, ..
                },
            ) = group.tx_rx_dc(&client).await.expect("TX/RX");

            for mut slave in group.iter(&client) {
                let (_i, o) = slave.io_raw_mut();

                for byte in o.iter_mut() {
                    *byte = byte.wrapping_add(1);
                }
            }

            smol::Timer::at(now + next_cycle_wait).await;

            if term.load(Ordering::Relaxed) {
                log::info!("Exiting...");

                break;
            }
        }

        let group = group.into_safe_op(&client).await.expect("OP -> SAFE-OP");

        log::info!("OP -> SAFE-OP");

        let group = group.into_pre_op(&client).await.expect("SAFE-OP -> PRE-OP");

        log::info!("SAFE-OP -> PRE-OP");

        let _group = group.into_init(&client).await.expect("PRE-OP -> INIT");

        log::info!("PRE-OP -> INIT, shutdown complete");

        Ok(())
    })
}
