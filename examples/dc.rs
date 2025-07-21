//! Configure Distributed Clocks (DC) for EK1100 and a couple of other modules.
//!
//! Please note this example uses experimental features and should not be used as a reference for
//! other code. It is here (currently) primarily to help develop EtherCrab.

use env_logger::Env;
use ethercrab::{
    DcSync, MainDevice, MainDeviceConfig, PduStorage, RegisterAddress, Timeouts,
    error::Error,
    std::{ethercat_now, tx_rx_task},
    subdevice_group::{CycleInfo, DcConfiguration, TxRxResponse},
};
use futures_lite::StreamExt;
use smol::LocalExecutor;
use std::{
    fs::File,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};
use ta::Next;
use ta::indicators::ExponentialMovingAverage;
use thread_priority::{
    RealtimeThreadSchedulePolicy, ThreadPriority, ThreadPriorityValue, ThreadSchedulePolicy,
};

/// Maximum number of SubDevices that can be stored. This must be a power of 2 greater than 1.
const MAX_SUBDEVICES: usize = 16;
const MAX_PDU_DATA: usize = PduStorage::element_size(1100);
const MAX_FRAMES: usize = 32;
const PDI_LEN: usize = 512;

static PDU_STORAGE: PduStorage<MAX_FRAMES, MAX_PDU_DATA> = PduStorage::new();

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

const TICK_INTERVAL: Duration = Duration::from_millis(5);

fn main() -> Result<(), Error> {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    let interface = std::env::args()
        .nth(1)
        .expect("Provide network interface as first argument.");

    log::info!("Starting Distributed Clocks demo...");
    log::info!("Run with RUST_LOG=ethercrab=debug or =trace for debug information");

    let (tx, rx, pdu_loop) = PDU_STORAGE.try_split().expect("can only split once");

    let maindevice = Arc::new(MainDevice::new(
        pdu_loop,
        Timeouts {
            wait_loop_delay: Duration::from_millis(5),
            state_transition: Duration::from_secs(10),
            pdu: Duration::from_millis(2000),
            ..Timeouts::default()
        },
        MainDeviceConfig {
            dc_static_sync_iterations: 10_000,
            ..MainDeviceConfig::default()
        },
    ));

    let mut tick_interval = smol::Timer::interval(TICK_INTERVAL);

    thread_priority::ThreadBuilder::default()
        .name("tx-rx-task")
        // Might need to set `<user> hard rtprio 99` and `<user> soft rtprio 99` in `/etc/security/limits.conf`
        // Check limits with `ulimit -Hr` or `ulimit -Sr`
        .priority(ThreadPriority::Crossplatform(
            ThreadPriorityValue::try_from(99u8).unwrap(),
        ))
        // NOTE: Requires a realtime kernel
        .policy(ThreadSchedulePolicy::Realtime(
            RealtimeThreadSchedulePolicy::Fifo,
        ))
        .spawn(move |_| {
            core_affinity::set_for_current(core_affinity::CoreId { id: 0 })
                .then_some(())
                .expect("Set TX/RX thread core");

            let ex = LocalExecutor::new();

            futures_lite::future::block_on(
                ex.run(tx_rx_task(&interface, tx, rx).expect("spawn TX/RX task")),
            )
            .expect("TX/RX task exited");
        })
        .unwrap();

    // Wait for TX/RX loop to start
    thread::sleep(Duration::from_millis(200));

    #[cfg(target_os = "linux")]
    thread_priority::set_current_thread_priority(thread_priority::ThreadPriority::Crossplatform(
        thread_priority::ThreadPriorityValue::try_from(48u8).unwrap(),
    ))
    .expect("Main thread prio");

    smol::block_on(async {
        let mut group = maindevice
            .init_single_group::<MAX_SUBDEVICES, PDI_LEN>(ethercat_now)
            .await
            .expect("Init");

        // The group will be in PRE-OP at this point

        for mut subdevice in group.iter_mut(&maindevice) {
            if subdevice.name() == "LAN9252-EVB-HBI" {
                // Sync mode 02 = SYNC0
                subdevice
                    .sdo_write(0x1c32, 1, 2u16)
                    .await
                    .expect("Set sync mode");

                // ETG1020 calc and copy time
                let cal_and_copy_time = subdevice
                    .sdo_read::<u16>(0x1c32, 6)
                    .await
                    .expect("Calc and copy time");

                // Delay time
                let delay_time = subdevice
                    .sdo_read::<u16>(0x1c32, 9)
                    .await
                    .expect("Delay time");

                log::info!(
                    "LAN9252 calc time {} ns, delay time {} ns",
                    cal_and_copy_time,
                    delay_time,
                );

                // Adding this seems to make the second LAN9252 converge much more quickly
                subdevice
                    .sdo_write(0x1c32, 0x0a, TICK_INTERVAL.as_nanos() as u32)
                    .await
                    .expect("Set cycle time");

                let sync_type = subdevice.sdo_read::<u16>(0x1c32, 1).await?;
                let cycle_time = subdevice.sdo_read::<u32>(0x1c32, 2).await?;
                let min_cycle_time = subdevice.sdo_read::<u32>(0x1c32, 5).await?;
                let supported_sync_modes = subdevice.sdo_read::<SupportedModes>(0x1c32, 4).await?;
                log::info!(
                    "--> Outputs sync mode {sync_type}, cycle time {cycle_time} ns (min {min_cycle_time} ns), supported modes {supported_sync_modes:?}"
                );

                let sync_type = subdevice.sdo_read::<u16>(0x1c33, 1).await?;
                let cycle_time = subdevice.sdo_read::<u32>(0x1c33, 2).await?;
                let min_cycle_time = subdevice.sdo_read::<u32>(0x1c33, 5).await?;
                let supported_sync_modes = subdevice.sdo_read::<SupportedModes>(0x1c33, 4).await?;
                log::info!(
                    "--> Inputs sync mode {sync_type}, cycle time {cycle_time} ns (min {min_cycle_time} ns), supported modes {supported_sync_modes:?}"
                );
            }

            // Configure SYNC0 AND SYNC1 for EL4102
            if subdevice.name() == "EL4102" {
                log::info!("Found EL4102");

                // Sync mode 02 = SYNC0
                subdevice
                    .sdo_write(0x1c32, 1, 2u16)
                    .await
                    .expect("Set sync mode");

                subdevice
                    .sdo_write(0x1c32, 0x02, TICK_INTERVAL.as_nanos() as u32)
                    .await
                    .expect("Set cycle time");

                // ETG1020 calc and copy time
                let cal_and_copy_time = subdevice
                    .sdo_read::<u16>(0x1c32, 6)
                    .await
                    .expect("Calc and copy time");

                // Delay time
                let delay_time = subdevice
                    .sdo_read::<u16>(0x1c32, 9)
                    .await
                    .expect("Delay time");

                log::info!(
                    "--> Calc time {} ns, delay time {} ns",
                    cal_and_copy_time,
                    delay_time,
                );

                let sync_type = subdevice.sdo_read::<u16>(0x1c32, 1).await?;
                let cycle_time = subdevice.sdo_read::<u32>(0x1c32, 2).await?;
                let shift_time = subdevice.sdo_read::<u32>(0x1c32, 3).await?;
                let min_cycle_time = subdevice.sdo_read::<u32>(0x1c32, 5).await?;
                let supported_sync_modes = subdevice.sdo_read::<SupportedModes>(0x1c32, 4).await?;
                // NOTE: For EL4102, SupportedModes.sync1 is false, but the ESI file specifies it,
                // and the 4102 won't go into OP without setting up SYNC1 with the correct offset.
                // Brilliant.
                log::info!(
                    "--> Outputs sync mode {sync_type}, cycle time {cycle_time} ns (min {min_cycle_time} ns), shift {shift_time} ns, supported modes {supported_sync_modes:?}"
                );

                subdevice.set_dc_sync(DcSync::Sync01 {
                    // EL4102 ESI specifies SYNC1 with an offset of 100k ns
                    sync1_period: Duration::from_nanos(100_000),
                });
            } else {
                // Enable SYNC0 for any other SubDevice kind
                subdevice.set_dc_sync(DcSync::Sync0);
            }
        }

        log::info!("Group has {} SubDevices", group.len());

        let mut averages = Vec::new();

        for _ in 0..group.len() {
            averages.push(ExponentialMovingAverage::new(64).unwrap());
        }

        log::info!("Moving into PRE-OP with PDI");

        let group = group.into_pre_op_pdi(&maindevice).await?;

        log::info!("Done. PDI available. Waiting for SubDevices to align");

        let mut align_stats = {
            let mut w = csv::Writer::from_writer(File::create("dc-align.csv").expect("Open CSV"));

            w.write_field("t_ms").ok();

            for s in group.iter(&maindevice) {
                w.write_field(format!("{:#06x}", s.configured_address()))
                    .ok();
                w.write_field(format!("{:#06x} EMA", s.configured_address()))
                    .ok();
            }

            // Finish header
            w.write_record(None::<&[u8]>).ok();

            w
        };

        let mut now = Instant::now();
        let start = Instant::now();

        // Repeatedly send group PDI and sync frame to align all SubDevice clocks. We use an
        // exponential moving average of each SubDevice's deviation from the EtherCAT System Time
        // (the time in the DC reference SubDevice) and take the maximum deviation. When that is
        // below 100ns (arbitraily chosen value for this demo), we call the sync good enough and
        // exit the loop.
        loop {
            group
                .tx_rx_sync_system_time(&maindevice)
                .await
                .expect("TX/RX");

            align_stats
                .write_field(start.elapsed().as_millis().to_string())
                .ok();

            let mut max_deviation = 0;

            for (s1, ema) in group.iter(&maindevice).zip(averages.iter_mut()) {
                let diff = match s1
                    .register_read::<u32>(RegisterAddress::DcSystemTimeDifference)
                    .await
                {
                    Ok(value) =>
                    // The returned value is NOT in two's compliment, rather the upper bit specifies
                    // whether the number in the remaining bits is odd or even, so we convert the
                    // value to `i32` using that logic here.
                    {
                        let flag = 0b1u32 << 31;

                        if value >= flag {
                            // Strip off negative flag bit and negate value as normal
                            -((value & !flag) as i32)
                        } else {
                            value as i32
                        }
                    }
                    Err(Error::WorkingCounter { .. }) => 0,
                    Err(e) => return Err(e),
                };

                let ema_next = ema.next(diff as f64);

                max_deviation = max_deviation.max(ema_next.abs() as u32);

                align_stats.write_field(diff.to_string()).ok();
                align_stats.write_field(ema_next.to_string()).ok();
            }

            // Finish row
            align_stats.write_record(None::<&[u8]>).ok();

            if now.elapsed() >= Duration::from_millis(1000) {
                now = Instant::now();

                log::info!("--> Max deviation {} ns", max_deviation);

                // Less than 500ns max deviation as an example threshold.
                // <https://github.com/OpenEtherCATsociety/SOEM/issues/487#issuecomment-786245585>
                // mentions less than 100us as a good enough value as well.
                if max_deviation < 500 {
                    log::info!("Clocks settled after {} ms", start.elapsed().as_millis());

                    break;
                }
            }

            tick_interval.next().await;
        }

        align_stats.flush().ok();

        log::info!("Alignment done");

        // SubDevice clocks are aligned. We can turn DC on now.
        let group = group
            .configure_dc_sync(
                &maindevice,
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
            .into_safe_op(&maindevice)
            .await
            .expect("PRE-OP -> SAFE-OP");

        log::info!("SAFE-OP");

        // Request OP state without waiting for all SubDevices to reach it. Allows the immediate
        // start of the process data cycle, which is required when DC sync is used, otherwise
        // SubDevices never reach OP, most often timing out with a SyncManagerWatchdog error.
        let group = group
            .request_into_op(&maindevice)
            .await
            .expect("SAFE-OP -> OP");

        log::info!("OP requested");

        let op_request = Instant::now();

        // Send PDI and check group state until all SubDevices enter OP state. At this point, we can
        // exit this loop and enter the main process data loop that does not have the state check
        // overhead present here.
        loop {
            let now = Instant::now();

            let response @ TxRxResponse {
                working_counter: _wkc,
                extra: CycleInfo {
                    next_cycle_wait, ..
                },
                ..
            } = group.tx_rx_dc(&maindevice).await.expect("TX/RX");

            if response.all_op() {
                break;
            }

            smol::Timer::at(now + next_cycle_wait).await;
        }

        log::info!(
            "All SubDevices entered OP in {} us",
            op_request.elapsed().as_micros()
        );

        let term = Arc::new(AtomicBool::new(false));
        signal_hook::flag::register(signal_hook::consts::SIGINT, Arc::clone(&term))
            .expect("Register hook");

        let mut print_tick = Instant::now();

        // 16MB buffer to start with

        let mut process_stats = {
            // let mut w = csv::Writer::from_writer(File::create("dc-pd.csv").expect("Open CSV"));
            let pd_stats_buf = Vec::with_capacity(1024 * 1000 * 16);

            let mut w = csv::Writer::from_writer(pd_stats_buf);

            w.write_field("Elapsed (s)").ok();
            w.write_field("Cycle number").ok();
            w.write_field("DC time 32 bit (ns)").ok();
            // w.write_field("Next cycle wait (ns)").ok();

            for sd in group.iter(&maindevice) {
                if matches!(sd.dc_support(), ethercrab::DcSupport::RefOnly) {
                    continue;
                }

                w.write_field(format!("{:#06x} system time u32", sd.configured_address()))
                    .ok();
                w.write_field(format!("{:#06x} system time u64", sd.configured_address()))
                    .ok();
                w.write_field(format!(
                    "{:#06x} time to next sync0",
                    sd.configured_address()
                ))
                .ok();
                w.write_field(format!("{:#06x} next raw value", sd.configured_address()))
                    .ok();
            }

            // Finish header
            w.write_record(None::<&[u8]>).ok();

            w
        };

        let start = Instant::now();
        let mut cycle = 0;

        // Main application process data cycle
        loop {
            let now = Instant::now();

            let response @ TxRxResponse {
                working_counter: _wkc,
                extra:
                    CycleInfo {
                        dc_system_time,
                        next_cycle_wait,
                        cycle_start_offset,
                    },
                ..
            } = group.tx_rx_dc(&maindevice).await.expect("TX/RX");

            // Debug logging
            {
                let cycle_start_offset = cycle_start_offset.as_nanos() as u64;

                let should_print = print_tick.elapsed() > Duration::from_secs(1);

                // process_stats
                //     .write_field(start.elapsed().as_secs_f32().to_string())
                //     .ok();
                process_stats.write_field("").ok();
                process_stats.write_field(cycle.to_string()).ok();
                process_stats
                    .write_field((dc_system_time as u32).to_string())
                    .ok();

                if should_print {
                    print_tick = Instant::now();

                    log::info!(
                        "Offset from start of cycle {} ({:0.2} ms), next tick in {:0.3} ms, group status {:?}",
                        cycle_start_offset,
                        (cycle_start_offset as f32) / 1000.0 / 1000.0,
                        (next_cycle_wait.as_nanos() as f32) / 1000.0 / 1000.0,
                        response.group_state()
                    );
                }

                for sd in group.iter(&maindevice) {
                    if matches!(sd.dc_support(), ethercrab::DcSupport::RefOnly) {
                        continue;
                    }

                    let next_dc_sync_start_time = sd
                        .register_read::<u32>(RegisterAddress::DcSyncStartTime)
                        .await
                        .unwrap_or_default();

                    let sd_time_32 = sd
                        .register_read::<u32>(RegisterAddress::DcSystemTime)
                        .await?;
                    let sd_time_64 = sd
                        .register_read::<u64>(RegisterAddress::DcSystemTime)
                        .await?;

                    let next_sync0 = (next_dc_sync_start_time - sd_time_32) as f64 / 1_000_000.;
                    process_stats.write_field(sd_time_32.to_string()).ok();
                    process_stats.write_field(sd_time_64.to_string()).ok();
                    process_stats.write_field(next_sync0.to_string()).ok();
                    process_stats
                        .write_field(next_dc_sync_start_time.to_string())
                        .ok();

                    if should_print {
                        log::info!(
                            "{:#06x}, next sync0 in: {} ms, 32b t {}, {}, 64b t {}",
                            sd.configured_address(),
                            next_sync0,
                            sd_time_32,
                            next_dc_sync_start_time,
                            sd_time_64,
                        );
                    }
                }

                // Finish row
                let _ = process_stats.write_record(None::<&[u8]>);
            }

            for subdevice in group.iter(&maindevice) {
                let mut o = subdevice.outputs_raw_mut();

                for byte in o.iter_mut() {
                    *byte = byte.wrapping_add(1);
                }
            }

            smol::Timer::at(now + next_cycle_wait).await;

            cycle += 1;

            // Hook signal so we can write CSV data before exiting
            if term.load(Ordering::Relaxed) {
                log::info!("Exiting...");

                process_stats.flush().ok();

                break;
            }
        }

        let _ = std::fs::write("dc-pd.csv", process_stats.into_inner().unwrap());

        let group = group
            .into_safe_op(&maindevice)
            .await
            .expect("OP -> SAFE-OP");

        log::info!("OP -> SAFE-OP");

        let group = group
            .into_pre_op(&maindevice)
            .await
            .expect("SAFE-OP -> PRE-OP");

        log::info!("SAFE-OP -> PRE-OP");

        let _group = group.into_init(&maindevice).await.expect("PRE-OP -> INIT");

        log::info!("PRE-OP -> INIT, shutdown complete");

        Ok(())
    })
}
