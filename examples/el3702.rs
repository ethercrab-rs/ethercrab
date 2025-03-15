//! Configure Distributed Clocks (DC) for EK1100 and a couple of other modules.
//!
//! Please note this example uses experimental features and should not be used as a reference for
//! other code. It is here (currently) primarily to help develop EtherCrab.

use env_logger::Env;
use ethercrab::{
    error::Error,
    std::ethercat_now,
    subdevice_group::{CycleInfo, DcConfiguration, TxRxResponse},
    DcSync, MainDevice, MainDeviceConfig, PduStorage, RegisterAddress, Timeouts,
};
use futures_lite::StreamExt;
use std::{
    fs::File,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};
use ta::indicators::ExponentialMovingAverage;
use ta::Next;

/// Maximum number of SubDevices that can be stored. This must be a power of 2 greater than 1.
const MAX_SUBDEVICES: usize = 16;
const MAX_PDU_DATA: usize = PduStorage::element_size(1100);
const MAX_FRAMES: usize = 32;
const PDI_LEN: usize = 256;

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

const TICK_INTERVAL: Duration = Duration::from_millis(1);

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
            dc_static_sync_iterations: 1_000,
            ..MainDeviceConfig::default()
        },
    ));

    let mut tick_interval = smol::Timer::interval(TICK_INTERVAL);

    smol::spawn(ethercrab::std::tx_rx_task(&interface, tx, rx).expect("spawn TX/RX task")).detach();

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
            println!("Found device: {}", subdevice.name());
            // Configure SYNC0 AND SYNC1 for EL4732 and EL3702
            if subdevice.name() == "EL4732" {
                subdevice.set_dc_sync(DcSync::Sync01 {
                    sync1_period: Duration::from_millis(1),
                });
            } else if subdevice.name() == "EL3702" {
                subdevice.set_dc_sync(DcSync::Sync01 {
                    sync1_period: Duration::from_micros(500),
                });
            } else {
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

            if now.elapsed() >= Duration::from_millis(25) {
                now = Instant::now();

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

                log::debug!("--> Max deviation {} ns", max_deviation);

                // Less than 100ns max deviation as an example threshold.
                // <https://github.com/OpenEtherCATsociety/SOEM/issues/487#issuecomment-786245585>
                // mentions less than 100us as a good enough value as well.
                if max_deviation < 100 {
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

        #[derive(serde::Serialize)]
        struct ProcessStat {
            ecat_time: u64,
            cycle_start_offset: u64,
            next_iter_wait: u64,
        }

        let mut process_stats =
            csv::Writer::from_writer(File::create("dc-pd.csv").expect("Open CSV"));

        let mut print_tick = Instant::now();

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

                let stat = ProcessStat {
                    ecat_time: dc_system_time,
                    next_iter_wait: next_cycle_wait.as_nanos() as u64,
                    cycle_start_offset,
                };

                if print_tick.elapsed() > Duration::from_secs(1) {
                    print_tick = Instant::now();
                }

                process_stats.serialize(stat).ok();
            }

            //log::info!("Status: {:?}", response.group_state());

            for subdevice in group.iter(&maindevice) {
                if subdevice.name().contains("EL4732") {
                    let mut o = subdevice.outputs_raw_mut();
                    let voltage = -12.0_f64;

                    // Convert voltage from -10V to +10V to a signed 16-bit value (-32768 to +32767)
                    let normalized = voltage / 10.0_f64; // Normalize to -1.0 to +1.0 range
                    let dac_value = (normalized * 32767.0_f64).round() as i16;

                    // Extract bytes from the signed value
                    // For little-endian (low byte first)
                    o[2] = (dac_value & 0xFF) as u8;
                    o[3] = ((dac_value >> 8) & 0xFF) as u8;

                    // For big-endian (high byte first)
                    // o[2] = ((dac_value >> 8) & 0xFF) as u8;
                    // o[3] = (dac_value & 0xFF) as u8;
                } else if subdevice.name().contains("EL3702") {
                    let io = subdevice.io_raw();
                    log::info!("EL3702 complete input data: {:02X?}", io.inputs());

                    let ch1_raw = i16::from_le_bytes([io.inputs()[2], io.inputs()[3]]);
                    let ch2_raw = i16::from_le_bytes([io.inputs()[6], io.inputs()[7]]);

                    // Convert to voltage (+/-10V range)
                    let ch1_voltage = (ch1_raw as f32 / 32768.0) * 10.0;
                    let ch2_voltage = (ch2_raw as f32 / 32768.0) * 10.0;

                    //log::info!(
                    //    "EL3702 Inputs - CH1: {:+7.3} V (raw: {:+6}), CH2: {:+7.3} V (raw: {:+6})",
                    //    ch1_voltage, ch1_raw, ch2_voltage, ch2_raw
                    //);
                }
            }

            smol::Timer::at(now + next_cycle_wait).await;

            // Hook signal so we can write CSV data before exiting
            if term.load(Ordering::Relaxed) {
                log::info!("Exiting...");

                process_stats.flush().ok();

                break;
            }
        }

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
