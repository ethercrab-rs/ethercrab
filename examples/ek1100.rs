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
    std::{ethercat_now, tx_rx_task},
    MainDevice, MainDeviceConfig, PduStorage, Timeouts,
};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
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

#[tokio::main]
async fn main() -> Result<(), Error> {
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
            ..Default::default()
        },
        MainDeviceConfig::default(),
    ));

    tokio::spawn(tx_rx_task(&interface, tx, rx).expect("spawn TX/RX task"));

    let mut group = maindevice
        .init_single_group::<MAX_SUBDEVICES, PDI_LEN>(ethercat_now)
        .await
        .expect("Init");

    log::info!("Discovered {} SubDevices", group.len());

    for subdevice in group.iter(&maindevice) {
        if subdevice.name() == "EL3004" {
            log::info!("Found EL3004. Configuring...");

            subdevice.sdo_write(0x1c12, 0, 0u8).await?;
            subdevice.sdo_write(0x1c13, 0, 0u8).await?;

            subdevice.sdo_write(0x1c13, 1, 0x1a00u16).await?;
            subdevice.sdo_write(0x1c13, 2, 0x1a02u16).await?;
            subdevice.sdo_write(0x1c13, 3, 0x1a04u16).await?;
            subdevice.sdo_write(0x1c13, 4, 0x1a06u16).await?;
            subdevice.sdo_write(0x1c13, 0, 4u8).await?;
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

    let mut tick_interval = tokio::time::interval(Duration::from_millis(5));
    tick_interval.set_missed_tick_behavior(MissedTickBehavior::Skip);

    let shutdown = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(signal_hook::consts::SIGINT, Arc::clone(&shutdown))
        .expect("Register hook");

    loop {
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

        tick_interval.tick().await;
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
