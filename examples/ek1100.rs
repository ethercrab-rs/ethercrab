//! Demonstrate setting outputs using a Beckhoff EK1100/EK1501 and modules.
//!
//! Run with e.g.
//!
//! Linux
//!
//! ```bash
//! RUST_LOG=debug cargo run --example ek1100 --release -- eth0
//! ```
//!
//! Windows
//!
//! ```ps
//! $env:RUST_LOG="debug" ; cargo run --example ek1100 --release -- '\Device\NPF_{FF0ACEE6-E8CD-48D5-A399-619CD2340465}'
//! ```

use env_logger::Env;
use ethercrab::{
    error::Error,
    std::{ethercat_now, tx_rx_task_blocking},
    MainDevice, MainDeviceConfig, PduStorage, Timeouts,
};
use futures_lite::StreamExt;
use spin_sleep::{SpinSleeper, SpinStrategy};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use thread_priority::{ThreadPriority, ThreadPriorityValue};
use tokio::time::MissedTickBehavior;

/// Maximum number of SubDevices that can be stored. This must be a power of 2 greater than 1.
const MAX_SUBDEVICES: usize = 16;
/// Maximum PDU data payload size - set this to the max PDI size or higher.
const MAX_PDU_DATA: usize = PduStorage::element_size(1100);
/// Maximum number of EtherCAT frames that can be in flight at any one time.
const MAX_FRAMES: usize = 16;
/// Maximum total PDI length.
const PDI_LEN: usize = 64;

static PDU_STORAGE: PduStorage<MAX_FRAMES, MAX_PDU_DATA> = PduStorage::new();

fn main() -> Result<(), Error> {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    let interface = std::env::args()
        .nth(1)
        .expect("Provide network interface as first argument.");

    log::info!("Starting EK1100/EK1501 demo...");
    log::info!(
        "Ensure an EK1100 or EK1501 is the first SubDevice, with any number of modules connected after"
    );
    log::info!("Run with RUST_LOG=ethercrab=debug or =trace for debug information");

    let (tx, rx, pdu_loop) = PDU_STORAGE.try_split().expect("can only split once");

    let maindevice = Arc::new(MainDevice::new(
        pdu_loop,
        Timeouts {
            wait_loop_delay: Duration::from_millis(2),
            mailbox_response: Duration::from_millis(1000),
            eeprom: Duration::from_millis(50),
            state_transition: Duration::from_secs(20),
            pdu: Duration::from_millis(50),
            ..Default::default()
        },
        MainDeviceConfig {
            dc_static_sync_iterations: 1000,
            ..MainDeviceConfig::default()
        },
    ));

    let core_ids = core_affinity::get_core_ids().unwrap();

    // Pick the non-HT cores
    let main_thread_core = core_ids[0];
    let tx_rx_core = core_ids[2];

    core_affinity::set_for_current(main_thread_core);

    thread_priority::ThreadBuilder::default()
        .name("tx-rx-thread")
        // Might need to set `<user> hard rtprio 99` and `<user> soft rtprio 99` in `/etc/security/limits.conf`
        // Check limits with `ulimit -Hr` or `ulimit -Sr`
        // .priority(ThreadPriority::Crossplatform(
        //     ThreadPriorityValue::try_from(49u8).unwrap(),
        // ))
        // // NOTE: Requires a realtime kernel
        // .policy(ThreadSchedulePolicy::Realtime(
        //     RealtimeThreadSchedulePolicy::Fifo,
        // ))
        .spawn(move |_| {
            core_affinity::set_for_current(tx_rx_core)
                .then_some(())
                .expect("Set TX/RX thread core");

            let local = smol::LocalExecutor::new();

            tx_rx_task_blocking(&interface, tx, rx).expect("TX/RX task");
        })
        .unwrap();

    // smol::spawn(tx_rx_task(&interface, tx, rx).expect("spawn TX/RX task")).detach();

    smol::block_on(async {
        let mut group = maindevice
            .init_single_group::<MAX_SUBDEVICES, PDI_LEN>(ethercat_now)
            .await
            .expect("Init");

        log::info!("Discovered {} SubDevices", group.len());

        for subdevice in group.iter(&maindevice) {
            if subdevice.name() == "EL3004" {
                log::info!("Found EL3004. Configuring...");

                subdevice.sdo_write(0x1c12, 0, 0u8).await?;

                subdevice
                    .sdo_write_array(0x1c13, &[0x1a00u16, 0x1a02, 0x1a04, 0x1a06])
                    .await?;

                // The `sdo_write_array` call above is equivalent to the following
                // subdevice.sdo_write(0x1c13, 0, 0u8).await?;
                // subdevice.sdo_write(0x1c13, 1, 0x1a00u16).await?;
                // subdevice.sdo_write(0x1c13, 2, 0x1a02u16).await?;
                // subdevice.sdo_write(0x1c13, 3, 0x1a04u16).await?;
                // subdevice.sdo_write(0x1c13, 4, 0x1a06u16).await?;
                // subdevice.sdo_write(0x1c13, 0, 4u8).await?;
            }
        }

        let mut group = group.into_op(&maindevice).await.expect("PRE-OP -> OP");

        for subdevice in group.iter(&maindevice) {
            let (i, o) = subdevice.io_raw();

            log::info!(
                "-> SubDevice {:#06x} {} inputs: {} bytes, outputs: {} bytes",
                subdevice.configured_address(),
                subdevice.name(),
                i.len(),
                o.len()
            );
        }

        // let mut tick_interval = tokio::time::interval(Duration::from_millis(5));
        // tick_interval.set_missed_tick_behavior(MissedTickBehavior::Skip);

        let mut tick_interval = smol::Timer::interval(Duration::from_millis(5));

        let sleeper = SpinSleeper::default().with_spin_strategy(SpinStrategy::SpinLoopHint);

        let mut clock = quanta::Clock::new();

        let shutdown = Arc::new(AtomicBool::new(false));
        signal_hook::flag::register(signal_hook::consts::SIGINT, Arc::clone(&shutdown))
            .expect("Register hook");

        // Run for 10 seconds
        for _ in 0..2_000 {
            let now = clock.now();

            // Graceful shutdown on Ctrl + C
            if shutdown.load(Ordering::Relaxed) {
                log::info!("Shutting down...");

                break;
            }

            group.tx_rx(&maindevice).await.expect("TX/RX");

            // Increment every output byte for every SubDevice by one
            for mut subdevice in group.iter(&maindevice) {
                let (_i, o) = subdevice.io_raw_mut();

                for byte in o.iter_mut() {
                    *byte = byte.wrapping_add(1);
                }
            }

            let wait = Duration::from_millis(5).saturating_sub(now.elapsed());

            // tick_interval.next().await;

            sleeper.sleep(wait);
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
