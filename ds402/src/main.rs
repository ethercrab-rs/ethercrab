//! Configure Distributed Clocks (DC) for EK1100 and a couple of other modules.
//!
//! Please note this example uses experimental features and should not be used as a reference for
//! other code. It is here (currently) primarily to help develop EtherCrab.

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
    thread,
    time::{Duration, Instant},
};
use ta::indicators::ExponentialMovingAverage;
use ta::Next;

/// Maximum number of slaves that can be stored. This must be a power of 2 greater than 1.
const MAX_SLAVES: usize = 16;
const MAX_PDU_DATA: usize = PduStorage::element_size(1100);
const MAX_FRAMES: usize = 32;
const PDI_LEN: usize = 64;

static PDU_STORAGE: PduStorage<MAX_FRAMES, MAX_PDU_DATA> = PduStorage::new();

const TICK_INTERVAL: Duration = Duration::from_millis(2);

fn main() -> Result<(), Error> {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    let interface = std::env::args()
        .nth(1)
        .expect("Provide network interface as first argument.");

    let (tx, rx, pdu_loop) = PDU_STORAGE.try_split().expect("can only split once");

    let client = Arc::new(Client::new(
        pdu_loop,
        Timeouts::default(),
        ClientConfig::default(),
    ));

    let mut tick_interval = smol::Timer::interval(TICK_INTERVAL);

    smol::spawn(tx_rx_task(&interface, tx, rx).expect("spawn TX/RX task")).detach();

    // Wait for TX/RX loop to start
    thread::sleep(Duration::from_millis(200));

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

        // The group will be in PRE-OP at this point

        for mut slave in group.iter(&client) {
            // Enable SYNC0
            slave.set_dc_sync(DcSync::Sync0);
        }

        log::info!("Group has {} slaves", group.len());

        let mut averages = Vec::new();

        for _ in 0..group.len() {
            averages.push(ExponentialMovingAverage::new(64).unwrap());
        }

        log::info!("Moving into PRE-OP with PDI");

        let mut group = group.into_pre_op_pdi(&client).await?;

        log::info!("Done. PDI available. Waiting for SubDevices to align");

        // Repeatedly send group PDI and sync frame to align all SubDevice clocks. We use an
        // exponential moving average of each SubDevice's deviation from the EtherCAT System Time
        // (the time in the DC reference SubDevice) and take the maximum deviation. When that is
        // below 100ns (arbitraily chosen value for this demo), we call the sync good enough and
        // exit the loop.
        loop {
            group.tx_rx_sync_system_time(&client).await.expect("TX/RX");

            let mut max_deviation = 0;

            for (s1, ema) in group.iter(&client).zip(averages.iter_mut()) {
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
                break;
            }

            tick_interval.next().await;
        }

        log::info!("Alignment done");

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

        // Send PDI and check group state until all SubDevices enter OP state. At this point, we can
        // exit this loop and enter the main process data loop that does not have the state check
        // overhead present here.
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

        // Main application process data cycle
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

            // Hook exit signal so we can gracefully shutdown
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
