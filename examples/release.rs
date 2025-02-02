//! Demonstrate releasing of a global `PduLoop`, then of the PDU loop and all TX/RX handles so they
//! can be reused.
//!
//! The process data loop does 256 iterations then moves on to the next scenario.

// For Windows
#![allow(unused)]

use env_logger::Env;
use ethercrab::{
    error::Error, std::ethercat_now, MainDevice, MainDeviceConfig, PduStorage, Timeouts,
};
use std::time::Duration;
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

#[cfg(not(windows))]
#[tokio::main]
async fn main() -> Result<(), Error> {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    let interface = std::env::args()
        .nth(1)
        .expect("Provide network interface as first argument.");

    log::info!("Starting release demo...");
    log::info!("Run with RUST_LOG=ethercrab=debug or =trace for debug information");

    let (tx, rx, pdu_loop) = PDU_STORAGE.try_split().expect("can only split once");

    // ---
    // ---
    // First cycle: normal TX/RX
    // ---
    // ---

    let maindevice = MainDevice::new(pdu_loop, Timeouts::default(), MainDeviceConfig::default());

    let tx_rx_handle =
        tokio::spawn(ethercrab::std::tx_rx_task(&interface, tx, rx).expect("spawn TX/RX task"));

    process_loop(&maindevice).await;

    // SAFETY: Any groups created with the current `maindevice` MUST be dropped before this line.
    // They cannot be reused with a new `MainDevice` instance and must be initialised again.
    let pdu_loop = unsafe { maindevice.release() };

    // ---
    // ---
    // Second cycle: reuse TX/RX task and PDU loop, but create a new `MainDevice`.
    // ---
    // ---

    log::info!("PduLoop was released, starting new MainDevice...");

    // Now make a new MainDevice with the same PDU loop
    let maindevice = MainDevice::new(pdu_loop, Timeouts::default(), MainDeviceConfig::default());

    process_loop(&maindevice).await;

    // SAFETY: Any groups created with the current `maindevice` MUST be dropped before this line.
    // They cannot be reused with a new `MainDevice` instance and must be initialised again.
    let pdu_loop = unsafe { maindevice.release_all() };

    // ---
    // ---
    // Third cycle: stop the previous TX/RX task and create a new one with the now-released TX/RX
    // handles.
    // ---
    // ---

    let (tx, rx) = tx_rx_handle
        .await
        .expect("Failed to stop TX/RX task")
        .expect("TX/RX task error");

    log::info!("PduLoop, PduTx and PduRx were released, starting new TX/RX task and making new MainDevice...");

    // Now spawn a new TX/RX task. You could use a different network interface here, for example.
    let tx_rx_handle =
        tokio::spawn(ethercrab::std::tx_rx_task(&interface, tx, rx).expect("spawn TX/RX task"));

    let maindevice = MainDevice::new(pdu_loop, Timeouts::default(), MainDeviceConfig::default());

    process_loop(&maindevice).await;

    log::info!("Third PDU loop with second TX/RX task shutdown complete");

    // SAFETY: Any groups created with the current `maindevice` MUST be dropped before this line.
    // They cannot be reused with a new `MainDevice` instance and must be initialised again.
    let pdu_loop = unsafe { maindevice.release_all() };

    let (tx, rx) = tx_rx_handle
        .await
        .expect("Failed to stop TX/RX task")
        .expect("TX/RX task error");

    // ---
    // ---
    // Fourth cycle: do the same with `io_uring` (Linux only)
    // ---
    // ---
    #[cfg(target_os = "linux")]
    {
        use ethercrab::std::tx_rx_task_io_uring;

        log::info!("Linux only: reuse TX/RX with io_uring");

        // NOTE: This is a suboptimal TX/RX thread spawn. See the `io-uring` example for how to do it properly.
        let tx_rx_handle = std::thread::spawn(move || tx_rx_task_io_uring(&interface, tx, rx));

        let maindevice =
            MainDevice::new(pdu_loop, Timeouts::default(), MainDeviceConfig::default());

        process_loop(&maindevice).await;

        // SAFETY: Any groups created with the current `maindevice` MUST be dropped before this line.
        // They cannot be reused with a new `MainDevice` instance and must be initialised again.
        let _pdu_loop = unsafe { maindevice.release_all() };

        let (tx, rx) = tx_rx_handle
            .join()
            .expect("io_uring TX/RX thread")
            .expect("Could not recover TX/RX hadnles");

        // Handles are ready for reuse
        assert_eq!(tx.should_exit(), false);
        assert_eq!(rx.should_exit(), false);
    }

    Ok(())
}

async fn process_loop(maindevice: &MainDevice<'_>) {
    let group = maindevice
        .init_single_group::<MAX_SUBDEVICES, PDI_LEN>(ethercat_now)
        .await
        .expect("Init")
        .into_op(&maindevice)
        .await
        .expect("PRE-OP -> OP");

    log::info!("Discovered {} SubDevices", group.len());

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

    let mut tick_interval = tokio::time::interval(Duration::from_millis(5));
    tick_interval.set_missed_tick_behavior(MissedTickBehavior::Skip);

    for _ in 0..u8::MAX {
        group.tx_rx(&maindevice).await.expect("TX/RX");

        // Increment every output byte for every SubDevice by one
        for subdevice in group.iter(&maindevice) {
            let mut o = subdevice.outputs_raw_mut();

            for byte in o.iter_mut() {
                *byte = byte.wrapping_add(1);
            }
        }

        tick_interval.tick().await;
    }

    let _ = group
        .into_safe_op(&maindevice)
        .await
        .expect("OP -> SAFE-OP")
        .into_pre_op(&maindevice)
        .await
        .expect("SAFE-OP -> PRE-OP")
        .into_init(&maindevice)
        .await
        .expect("PRE-OP -> INIT");
}

#[cfg(windows)]
fn main() {
    eprintln!("This example only supports non-Windows OSes");
}
