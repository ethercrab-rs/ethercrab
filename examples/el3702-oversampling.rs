//! Configure Distributed Clocks (DC) for EK1100 and a couple of other modules.
//!
//! Please note this example uses experimental features and should not be used as a reference for
//! other code. It is here (currently) primarily to help develop EtherCrab.

use env_logger::Env;
use ethercrab::{
    ds402::{self, Ds402, OpMode, PdoMapping, StatusWord, SyncManagerAssignment},
    error::Error,
    std::ethercat_now,
    subdevice_group::{CycleInfo, DcConfiguration, MappingConfig, TxRxResponse},
    DcSync, EtherCrabWireRead, EtherCrabWireWrite, MainDevice, MainDeviceConfig, PduStorage,
    RegisterAddress, Timeouts,
};
use futures_lite::StreamExt;
use std::{
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
const PDI_LEN: usize = 64;

static PDU_STORAGE: PduStorage<MAX_FRAMES, MAX_PDU_DATA> = PduStorage::new();

// This must remain at 1ms to get the drive into OP. The ESI file specifies this value.
const TICK_INTERVAL: Duration = Duration::from_millis(1);

fn main() -> Result<(), Error> {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    let interface = std::env::args()
        .nth(1)
        .expect("Provide network interface as first argument.");

    log::info!("Starting DS402 demo...");
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
            dc_static_sync_iterations: 1000,
            ..MainDeviceConfig::default()
        },
    ));

    let mut tick_interval = smol::Timer::interval(TICK_INTERVAL);

    #[cfg(target_os = "windows")]
    std::thread::spawn(move || {
        ethercrab::std::tx_rx_task_blocking(
            &interface,
            tx,
            rx,
            ethercrab::std::TxRxTaskConfig { spinloop: false },
        )
        .expect("TX/RX task")
    });
    #[cfg(not(target_os = "windows"))]
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

        log::info!("Group has {} SubDevices", group.len());

        let mut averages = Vec::new();

        for _ in 0..group.len() {
            averages.push(ExponentialMovingAverage::new(64).unwrap());
        }

        log::info!("Moving into PRE-OP with PDI");

        let el3702_mapping = MappingConfig::inputs(
            const {
                &[
                    SyncManagerAssignment::new(
                        const {
                            &[
                                // Ch1 cycle count
                                PdoMapping::new(
                                    0x1b00,
                                    const { &[PdoMapping::object::<u16>(0x6800, 1)] },
                                ),
                                // Ch1 first sample
                                PdoMapping::new(
                                    0x1a00,
                                    const { &[PdoMapping::object::<i16>(0x6000, 1)] },
                                )
                                .with_oversampling(2),
                            ]
                        },
                    )
                    .with_sync_manager(0)
                    .with_fmmu(0),
                    SyncManagerAssignment::new(
                        const {
                            &[
                                // Ch1 cycle count
                                PdoMapping::new(
                                    0x1b00,
                                    const { &[PdoMapping::object::<u16>(0x6800, 2)] },
                                ),
                                // Ch1 first sample
                                PdoMapping::new(
                                    0x1a00,
                                    const { &[PdoMapping::object::<i16>(0x6000, 2)] },
                                )
                                .with_oversampling(2),
                            ]
                        },
                    )
                    .with_sync_manager(1)
                    .with_fmmu(1),
                ]
            },
        );

        let group = group
            .into_pre_op_pdi_with_config(&maindevice, async |mut subdevice, idx| {
                if subdevice.name() == "EL3702" {
                    log::info!("Found EL3702 {:?}", subdevice.identity());

                    subdevice.set_dc_sync(DcSync::Sync01 {
                        sync1_period: Duration::from_millis(1),
                    });

                    Ok(Some(el3702_mapping))
                } else {
                    Ok(None)
                }
            })
            .await?;

        for sd in group.iter(&maindevice) {
            log::info!(
                "--> {:#06x} PDI {} input bytes, {} output bytes",
                sd.configured_address(),
                sd.inputs_raw().len(),
                sd.outputs_raw().len()
            );
        }

        log::info!("Done. PDI available. Waiting for SubDevices to align");

        let mut now = Instant::now();
        let start = Instant::now();

        // Repeatedly send group PDI and sync frame to align all SubDevice clocks. We use an
        // exponential moving average of each SubDevice's deviation from the EtherCAT System Time
        // (the time in the DC reference SubDevice) and take the maximum deviation. When that is
        // below 100us (arbitraily chosen value for this demo), we call the sync good enough and
        // exit the loop.
        loop {
            group
                .tx_rx_sync_system_time(&maindevice)
                .await
                .expect("TX/RX");

            if now.elapsed() >= Duration::from_millis(25) {
                now = Instant::now();

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
                }

                log::debug!("--> Max deviation {} ns", max_deviation);

                // Less than 100us max deviation
                if max_deviation < 100_000 {
                    log::info!("Clocks settled after {} ms", start.elapsed().as_millis());

                    break;
                }
            }

            tick_interval.next().await;
        }

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
                    // Taken from ESI file
                    sync0_shift: Duration::from_nanos(250_000),
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

        let mut sd = group.subdevice(&maindevice, 0)?;

        // Main application process data cycle
        loop {
            let now = Instant::now();

            let TxRxResponse {
                working_counter: _wkc,
                extra: CycleInfo {
                    next_cycle_wait, ..
                },
                ..
            } = group.tx_rx_dc(&maindevice).await.expect("TX/RX");

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

            if term.load(Ordering::Relaxed) {
                log::info!("Exiting...");

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
