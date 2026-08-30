//! Bring up a Beckhoff EK1100/EK1501 and modules on **Windows** and cycle process data.
//!
//! Windows-specific version of the `ek1100` example: the blocking Npcap-backed
//! [`tx_rx_task_blocking`](ethercrab::std::tx_rx_task_blocking) runs on its own OS thread (there
//! is no async TX/RX backend on Windows), `spin_sleep` + `quanta` drive the cycle timing (the
//! default Windows timer resolution of ~15 ms is too coarse), and `wait_loop_delay` is set to
//! `Duration::ZERO` to avoid spurious timeouts.
//!
//! Build/runtime setup (Npcap SDK + runtime, finding the `\Device\NPF_{...}` interface name):
//! see `doc/ek1100-windows.md`. Performance tuning: see `doc/windows-tuning.md`.
//!
//! Run with e.g.
//!
//! ```ps
//! $env:LIBPCAP_LIBDIR = 'C:\Npcap-SDK\Lib\x64'
//! $env:RUST_LOG = 'info'
//! cargo run --release --example ek1100-windows -- '\Device\NPF_{FF0ACEE6-E8CD-48D5-A399-619CD2340465}'
//! ```

#[cfg(windows)]
#[tokio::main]
async fn main() -> Result<(), ethercrab::error::Error> {
    use env_logger::Env;
    use ethercrab::{
        MainDevice, MainDeviceConfig, PduStorage, Timeouts,
        error::Error,
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
        .expect("Provide network interface as first argument, e.g. '\\Device\\NPF_{...}'.");

    log::info!("Starting EK1100/EK1501 demo (Windows)...");
    log::info!(
        "Ensure an EK1100 or EK1501 is the first SubDevice, with any number of modules connected after"
    );
    log::info!("Run with RUST_LOG=ethercrab=debug or =trace for debug information");

    let (tx, rx, pdu_loop) = PDU_STORAGE.try_split().expect("can only split once");

    let maindevice = Arc::new(MainDevice::new(
        pdu_loop,
        Timeouts {
            // Windows timers are coarse (~15ms min), which causes spurious timeouts if
            // `wait_loop_delay` is anything above zero.
            wait_loop_delay: Duration::ZERO,
            eeprom: Duration::from_millis(50),
            mailbox_response: Duration::from_millis(1000),
            ..Default::default()
        },
        MainDeviceConfig::default(),
    ));

    // The Windows TX/RX backend is blocking, so run it on a dedicated OS thread.
    std::thread::Builder::new()
        .name("tx-rx-task".to_string())
        .spawn(move || {
            tx_rx_task_blocking(&interface, tx, rx, TxRxTaskConfig { spinloop: false })
                .expect("TX/RX task");
        })
        .expect("spawn TX/RX thread");

    let group = maindevice
        .init_single_group::<MAX_SUBDEVICES, PDI_LEN>(ethercat_now)
        .await
        .expect("Init");

    log::info!("Discovered {} SubDevices", group.len());

    for subdevice in group.iter(&maindevice) {
        // Special case: if an EL3004 module is discovered, it needs some specific config during
        // init to function properly.
        if subdevice.name() == "EL3004" {
            log::info!("Found EL3004. Configuring...");

            subdevice.sdo_write(0x1c12, 0, 0u8).await?;

            subdevice
                .sdo_write_array(0x1c13, &[0x1a00u16, 0x1a02, 0x1a04, 0x1a06])
                .await?;
        }
    }

    let group = group.into_op(&maindevice).await.expect("PRE-OP -> OP");

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

    // A more accurate clock + spin sleeper for decent cycle timing on Windows.
    let clock = quanta::Clock::new();
    let sleeper = SpinSleeper::default().with_spin_strategy(SpinStrategy::SpinLoopHint);

    let shutdown = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(signal_hook::consts::SIGINT, Arc::clone(&shutdown))
        .expect("Register hook");

    log::info!("Cycling process data. Press Ctrl + C to stop.");

    loop {
        let now = clock.now();

        // Graceful shutdown on Ctrl + C.
        if shutdown.load(Ordering::Relaxed) {
            log::info!("Shutting down...");

            break;
        }

        group.tx_rx(&maindevice).await.expect("TX/RX");

        // Increment every output byte for every SubDevice by one.
        for subdevice in group.iter(&maindevice) {
            let mut o = subdevice.outputs_raw_mut();

            for byte in o.iter_mut() {
                *byte = byte.wrapping_add(1);
            }
        }

        let wait = cycle_time.saturating_sub(now.elapsed());
        sleeper.sleep(wait);
    }

    let group = group.into_safe_op(&maindevice).await.expect("OP -> SAFE-OP");
    log::info!("OP -> SAFE-OP");

    let group = group
        .into_pre_op(&maindevice)
        .await
        .expect("SAFE-OP -> PRE-OP");
    log::info!("SAFE-OP -> PRE-OP");

    let _group = group.into_init(&maindevice).await.expect("PRE-OP -> INIT");
    log::info!("PRE-OP -> INIT, shutdown complete");

    Ok::<(), Error>(())
}

#[cfg(not(windows))]
fn main() {
    eprintln!("This example is Windows-only. Use the `ek1100` example on other platforms.");
}
