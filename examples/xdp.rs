//! Run example with Distributed Clocks, using experimental XDP driver on Linux for better network
//! performance. The cycle time is 100us, which can be challenging for some machines to run, but is
//! easily possible on a Raspberry Pi 5 with some tuning.
//!
//! Requires a decent amount of Linux system tuning, including but not limited to:
//!
//! - PREEMPT-RT patches
//! - `tuned-adm profile realtime`
//! - `isolcpus=0`
//! - `ethtool -C enp2s0 tx-usecs 0 rx-usecs 0`
//! - `ethtool -A enp2s0 rx off tx off autoneg off`
//! - `ethtool -L enp2s0 combined 1`
//! - Setting IRQ affinity to the same core as the TX/RX task with e.g. `sudo sh -c "echo '1' >
//!   /proc/irq/124/smp_affinity"`
use core_affinity::CoreId;
use env_logger::Env;
use ethercrab::{
    DcSync, MainDevice, MainDeviceConfig, PduStorage, RegisterAddress, Timeouts, TxRxResponse,
    error::Error,
    std::{ethercat_now, tx_rx_task_xdp},
    subdevice_group::{CycleInfo, DcConfiguration},
};
use futures_lite::StreamExt;
use std::{
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

/// Maximum number of slaves that can be stored. This must be a power of 2 greater than 1.
const MAX_SUBDEVICES: usize = 16;
const MAX_PDU_DATA: usize = PduStorage::element_size(1100);
const MAX_FRAMES: usize = 32;
const PDI_LEN: usize = 64;

static PDU_STORAGE: PduStorage<MAX_FRAMES, MAX_PDU_DATA> = PduStorage::new();

const TICK_INTERVAL: Duration = Duration::from_micros(100);

fn main() -> Result<(), Error> {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    let interface = std::env::args()
        .nth(1)
        .expect("Provide network interface as first argument.");

    log::info!("Starting XDP demo...");
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
        .name("tx-rx-thread")
        // Might need to set `<user> hard rtprio 99` and `<user> soft rtprio 99` in `/etc/security/limits.conf`
        // Check limits with `ulimit -Hr` or `ulimit -Sr`
        .priority(ThreadPriority::Crossplatform(
            ThreadPriorityValue::try_from(49u8).unwrap(),
        ))
        // NOTE: Requires a realtime kernel
        .policy(ThreadSchedulePolicy::Realtime(
            RealtimeThreadSchedulePolicy::Fifo,
        ))
        .spawn(move |_| {
            // Works best if core is isolated with `isolcpus=0` boot param
            core_affinity::set_for_current(CoreId { id: 0 })
                .then_some(())
                .expect("Set TX/RX thread core");

            tx_rx_task_xdp(&interface, tx, rx).expect("TX/RX task");
            // ethercrab::std::tx_rx_task_io_uring(&interface, tx, rx).expect("TX/RX task");
        })
        .unwrap();

    // Wait for TX/RX loop to start
    thread::sleep(Duration::from_millis(200));

    // If the main thread is run on the same core as the XDP TX/RX thread, it will lock up, so pin
    // to core 1 to make sure this doesn't happen (TX/RX is pinned to core 0).
    core_affinity::set_for_current(CoreId { id: 1 })
        .then_some(())
        .expect("Set main task core");

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

        for mut slave in group.iter_mut(&maindevice) {
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
            }

            // Configure SYNC0 AND SYNC1 for EL4102
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

                slave.set_dc_sync(DcSync::Sync01 {
                    // EL4102 ESI specifies SYNC1 with an offset of 100k ns
                    sync1_period: Duration::from_nanos(100_000),
                });
            } else {
                // Enable SYNC0 for any other SubDevice kind
                slave.set_dc_sync(DcSync::Sync0);
            }
        }

        log::info!("Group has {} slaves", group.len());

        let mut averages = Vec::new();

        for _ in 0..group.len() {
            averages.push(ExponentialMovingAverage::new(64).unwrap());
        }

        log::info!("Moving into PRE-OP with PDI");

        let group = group.into_pre_op_pdi(&maindevice).await?;

        log::info!("Done. PDI available. Waiting for SubDevices to align");

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

        let term = Arc::new(AtomicBool::new(false));
        signal_hook::flag::register(signal_hook::consts::SIGINT, Arc::clone(&term))
            .expect("Register hook");

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

        // Main application process data cycle
        loop {
            let now = Instant::now();

            let TxRxResponse {
                working_counter: _wkc,
                extra:
                    CycleInfo {
                        next_cycle_wait,
                        cycle_start_offset,
                        ..
                    },
                ..
            } = group.tx_rx_dc(&maindevice).await.expect("TX/RX");

            // Debug logging
            {
                let cycle_start_offset = cycle_start_offset.as_nanos() as u64;

                if print_tick.elapsed() > Duration::from_secs(1) {
                    print_tick = Instant::now();

                    log::info!(
                        "Offset from start of cycle {} ({:0.2} ms), next tick in {:0.3} ms",
                        cycle_start_offset,
                        (cycle_start_offset as f32) / 1000.0 / 1000.0,
                        (next_cycle_wait.as_nanos() as f32) / 1000.0 / 1000.0
                    );
                }
            }

            for subdevice in group.iter(&maindevice) {
                for byte in subdevice.outputs_raw_mut().iter_mut() {
                    *byte = byte.wrapping_add(1);
                }
            }

            smol::Timer::at(now + next_cycle_wait).await;

            // Hook exit signal so we can shutdown gracefully
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
