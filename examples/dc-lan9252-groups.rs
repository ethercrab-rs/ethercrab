//! An extremely crude example that runs two DC-enabeld LAN9252 at different cycle times to check
//! SYNC0 alignment.

use env_logger::Env;
use ethercrab::{
    DcSync, MainDevice, MainDeviceConfig, PduStorage, RegisterAddress, SubDeviceGroup, Timeouts,
    error::Error,
    std::ethercat_now,
    subdevice_group::{CycleInfo, DcConfiguration, TxRxResponse},
};
use futures_lite::StreamExt;
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

/// Maximum number of SubDevices that can be stored. This must be a power of 2 greater than 1.
const MAX_SUBDEVICES: usize = 16;
const MAX_PDU_DATA: usize = PduStorage::element_size(1100);
const MAX_FRAMES: usize = 32;

static PDU_STORAGE: PduStorage<MAX_FRAMES, MAX_PDU_DATA> = PduStorage::new();

const SLOW_TICK_INTERVAL: Duration = Duration::from_millis(5);
const FAST_TICK_INTERVAL: Duration = Duration::from_micros(2500);

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
        Timeouts::default(),
        MainDeviceConfig::default(),
    ));

    let mut slow_tick_interval = smol::Timer::interval(SLOW_TICK_INTERVAL);
    let mut fast_tick_interval = smol::Timer::interval(SLOW_TICK_INTERVAL);

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

    #[cfg(target_os = "linux")]
    thread_priority::set_current_thread_priority(thread_priority::ThreadPriority::Crossplatform(
        thread_priority::ThreadPriorityValue::try_from(48u8).unwrap(),
    ))
    .expect("Main thread prio");

    smol::block_on(async {
        let (mut slow_group, mut fast_group) = maindevice
            .init::<MAX_SUBDEVICES, (SubDeviceGroup<1, 32>, SubDeviceGroup<1, 32>)>(
                ethercat_now,
                Default::default(),
                |groups, s| {
                    if s.configured_address() == 0x1000 {
                        Ok(&groups.0)
                    } else {
                        Ok(&groups.1)
                    }
                },
            )
            .await
            .expect("Init");

        for mut subdevice in slow_group.iter_mut(&maindevice) {
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
                .sdo_write(0x1c32, 0x0a, SLOW_TICK_INTERVAL.as_nanos() as u32)
                .await
                .expect("Set cycle time");

            subdevice.set_dc_sync(DcSync::Sync0);
        }

        for mut subdevice in fast_group.iter_mut(&maindevice) {
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
                .sdo_write(0x1c32, 0x0a, FAST_TICK_INTERVAL.as_nanos() as u32)
                .await
                .expect("Set cycle time");

            subdevice.set_dc_sync(DcSync::Sync0);
        }

        log::info!("Moving into PRE-OP with PDI");

        let slow_group = slow_group.into_pre_op_pdi(&maindevice).await?;
        let fast_group = fast_group.into_pre_op_pdi(&maindevice).await?;

        log::info!("Done. PDI available. Waiting for SubDevices to align");

        let mut now = Instant::now();
        let start = Instant::now();

        loop {
            slow_group
                .tx_rx_sync_system_time(&maindevice)
                .await
                .expect("TX/RX");

            if now.elapsed() >= Duration::from_millis(25) {
                now = Instant::now();

                let mut max_deviation = 0;

                for s1 in slow_group.iter(&maindevice) {
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

            slow_tick_interval.next().await;
        }

        log::info!("Slow group alignment done");

        let mut now = Instant::now();
        let start = Instant::now();

        loop {
            fast_group
                .tx_rx_sync_system_time(&maindevice)
                .await
                .expect("TX/RX");

            if now.elapsed() >= Duration::from_millis(25) {
                now = Instant::now();

                let mut max_deviation = 0;

                for s1 in fast_group.iter(&maindevice) {
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

            fast_tick_interval.next().await;
        }

        log::info!("Fast group alignment done");

        // SubDevice clocks are aligned. We can turn DC on now.
        let slow_group = slow_group
            .configure_dc_sync(
                &maindevice,
                DcConfiguration {
                    // Start SYNC0 100ms in the future
                    start_delay: Duration::from_millis(100),
                    // SYNC0 period should be the same as the process data loop in most cases
                    sync0_period: SLOW_TICK_INTERVAL,
                    // Send process data half way through cycle
                    sync0_shift: SLOW_TICK_INTERVAL / 2,
                },
            )
            .await?;

        let fast_group = fast_group
            .configure_dc_sync(
                &maindevice,
                DcConfiguration {
                    // Start SYNC0 100ms in the future
                    start_delay: Duration::from_millis(100),
                    // SYNC0 period should be the same as the process data loop in most cases
                    sync0_period: FAST_TICK_INTERVAL,
                    // Send process data half way through cycle
                    sync0_shift: FAST_TICK_INTERVAL / 2,
                },
            )
            .await?;

        let slow_group = slow_group
            .into_safe_op(&maindevice)
            .await
            .expect("PRE-OP -> SAFE-OP");

        let fast_group = fast_group
            .into_safe_op(&maindevice)
            .await
            .expect("PRE-OP -> SAFE-OP");

        log::info!("SAFE-OP");

        let term = Arc::new(AtomicBool::new(false));
        signal_hook::flag::register(signal_hook::consts::SIGINT, Arc::clone(&term))
            .expect("Register hook");

        // Request OP state without waiting for all SubDevices to reach it. Allows the immediate
        // start of the process data cycle, which is required when DC sync is used, otherwise
        // SubDevices never reach OP, most often timing out with a SyncManagerWatchdog error.
        let slow_group = slow_group
            .request_into_op(&maindevice)
            .await
            .expect("SAFE-OP -> OP");
        let fast_group = fast_group
            .request_into_op(&maindevice)
            .await
            .expect("SAFE-OP -> OP");

        log::info!("OP requested");

        let op_request = Instant::now();

        // Send PDI and check group state until all SubDevices enter OP state. At this point, we can
        // exit this loop and enter the main process data loop that does not have the state check
        // overhead present here.
        smol::future::race(
            async {
                loop {
                    let now = Instant::now();

                    let response @ TxRxResponse {
                        working_counter: _wkc,
                        extra:
                            CycleInfo {
                                next_cycle_wait, ..
                            },
                        ..
                    } = slow_group.tx_rx_dc(&maindevice).await.expect("TX/RX");

                    if response.all_op() {
                        break;
                    }

                    smol::Timer::at(now + next_cycle_wait).await;
                }

                Result::<_, Error>::Ok(())
            },
            async {
                loop {
                    let now = Instant::now();

                    let response @ TxRxResponse {
                        working_counter: _wkc,
                        extra:
                            CycleInfo {
                                next_cycle_wait, ..
                            },
                        ..
                    } = fast_group.tx_rx_dc(&maindevice).await.expect("TX/RX");

                    if response.all_op() {
                        break;
                    }

                    smol::Timer::at(now + next_cycle_wait).await;
                }

                Ok(())
            },
        )
        .await?;

        log::info!(
            "All SubDevices entered OP in {} us",
            op_request.elapsed().as_micros()
        );

        smol::future::race(
            async {
                loop {
                    let now = Instant::now();

                    let TxRxResponse {
                        working_counter: _wkc,
                        extra:
                            CycleInfo {
                                next_cycle_wait, ..
                            },
                        ..
                    } = slow_group.tx_rx_dc(&maindevice).await.expect("TX/RX");

                    for subdevice in slow_group.iter(&maindevice) {
                        let mut o = subdevice.outputs_raw_mut();

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
            },
            async {
                loop {
                    let now = Instant::now();

                    let TxRxResponse {
                        working_counter: _wkc,
                        extra:
                            CycleInfo {
                                next_cycle_wait, ..
                            },
                        ..
                    } = fast_group.tx_rx_dc(&maindevice).await.expect("TX/RX");

                    for subdevice in fast_group.iter(&maindevice) {
                        let mut o = subdevice.outputs_raw_mut();

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
            },
        )
        .await;

        let slow_group = slow_group
            .into_safe_op(&maindevice)
            .await
            .expect("OP -> SAFE-OP");
        let fast_group = fast_group
            .into_safe_op(&maindevice)
            .await
            .expect("OP -> SAFE-OP");

        log::info!("OP -> SAFE-OP");

        let slow_group = slow_group
            .into_pre_op(&maindevice)
            .await
            .expect("SAFE-OP -> PRE-OP");
        let fast_group = fast_group
            .into_pre_op(&maindevice)
            .await
            .expect("SAFE-OP -> PRE-OP");

        log::info!("SAFE-OP -> PRE-OP");

        let _slow_group = slow_group
            .into_init(&maindevice)
            .await
            .expect("PRE-OP -> INIT");
        let _fast_group = fast_group
            .into_init(&maindevice)
            .await
            .expect("PRE-OP -> INIT");

        log::info!("PRE-OP -> INIT, shutdown complete");

        Ok(())
    })
}
