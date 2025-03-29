//! A simple test program that demonstrates tweaks recommended to improve performance on Windows.
//!
//! Tested on Windows 11, i5-8500T, Intel i219-LM NIC. Your mileage may vary! Please open Github
//! issue with a Wireshark capture if you encounter perf issues.

#[cfg(windows)]
#[tokio::main]
async fn main() -> Result<(), ethercrab::error::Error> {
    use env_logger::Env;
    use ethercrab::{
        MainDevice, MainDeviceConfig, PduStorage, Timeouts,
        std::{TxRxTaskConfig, ethercat_now, tx_rx_task_blocking},
    };
    use spin_sleep::{SpinSleeper, SpinStrategy};
    use std::{
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
        time::Duration,
    };
    use thread_priority::{ThreadPriority, ThreadPriorityOsValue, WinAPIThreadPriority};

    /// Maximum number of SubDevices that can be stored. This must be a power of 2 greater than 1.
    const MAX_SUBDEVICES: usize = 16;
    /// Maximum PDU data payload size - set this to the max PDI size or higher.
    const MAX_PDU_DATA: usize = PduStorage::element_size(1100);
    /// Maximum number of EtherCAT frames that can be in flight at any one time.
    const MAX_FRAMES: usize = 16;
    /// Maximum total PDI length.
    const PDI_LEN: usize = 64;

    static PDU_STORAGE: PduStorage<MAX_FRAMES, MAX_PDU_DATA> = PduStorage::new();

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
            // Windows timers are rubbish (min delay is ~15ms) which will cause a bunch of timeouts
            // if `wait_loop_delay` is anything above 0.
            wait_loop_delay: Duration::ZERO,
            eeprom: Duration::from_millis(50),
            // Other timeouts can be left alone, or increased if other issues are found.
            ..Default::default()
        },
        MainDeviceConfig {
            // Quicker startup, mainly just for testing.
            dc_static_sync_iterations: 1000,
            ..MainDeviceConfig::default()
        },
    ));

    let core_ids = core_affinity::get_core_ids().expect("Get core IDs");

    // Pick the non-HT cores on my Intel i5-8500T test system. YMMV!
    let main_thread_core = core_ids[0];
    let tx_rx_core = core_ids[2];

    // Pinning this and the TX/RX thread reduce packet RTT spikes significantly
    core_affinity::set_for_current(main_thread_core);

    // Both `smol` and `tokio` use Windows' coarse timer, which has a resolution of at least
    // 15ms. This isn't useful for decent cycle times, so we use a more accurate clock from
    // `quanta` and a spin sleeper to get better timing accuracy.
    let sleeper = SpinSleeper::default().with_spin_strategy(SpinStrategy::SpinLoopHint);

    // NOTE: This takes ~200ms to return, so it must be called before any proper EtherCAT stuff
    // happens.
    let clock = quanta::Clock::new();

    // For best performance, use e.g.
    // https://www.techpowerup.com/download/microsoft-interrupt-affinity-tool/ to pin NIC IRQs to
    // the same core as the TX/RX thread.
    thread_priority::ThreadBuilder::default()
        .name("tx-rx-thread")
        // For best performance, this MUST be set if pinning NIC IRQs to the same core
        .priority(ThreadPriority::Os(ThreadPriorityOsValue::from(
            WinAPIThreadPriority::TimeCritical,
        )))
        .spawn(move |_| {
            core_affinity::set_for_current(tx_rx_core)
                .then_some(())
                .expect("Set TX/RX thread core");

            tx_rx_task_blocking(&interface, tx, rx, TxRxTaskConfig { spinloop: false })
                .expect("TX/RX task");
        })
        .unwrap();

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
        let io = subdevice.io_raw();

        log::info!(
            "-> SubDevice {:#06x} {} inputs: {} bytes, outputs: {} bytes",
            subdevice.configured_address(),
            subdevice.name(),
            io.inputs().len(),
            io.outputs().len()
        );
    }

    let cycle_time = Duration::from_millis(5);

    let shutdown = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(signal_hook::consts::SIGINT, Arc::clone(&shutdown))
        .expect("Register hook");

    loop {
        let now = clock.now();

        // Graceful shutdown on Ctrl + C
        if shutdown.load(Ordering::Relaxed) {
            log::info!("Shutting down...");

            break;
        }

        group.tx_rx(&maindevice).await.expect("TX/RX");

        // Increment every output byte for every SubDevice by one
        for mut subdevice in group.iter(&maindevice) {
            let mut o = subdevice.outputs_raw_mut();

            for byte in o.iter_mut() {
                *byte = byte.wrapping_add(1);
            }
        }

        let wait = cycle_time.saturating_sub(now.elapsed());
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
}

#[cfg(not(windows))]
fn main() {
    eprintln!(
        "Windows-only - the performance changes in this example don't make sense for other OSes"
    );
}
