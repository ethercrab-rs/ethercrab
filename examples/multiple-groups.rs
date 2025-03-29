//! Demonstrate sorting SubDevices into multiple SubDevice groups.
//!
//! This demo is designed to be used with the following SubDevices:
//!
//! - EK1100 (or EK1501 if using fibre)
//! - EL2889 (2 bytes of outputs)
//! - EL2828 (1 byte of outputs)

use env_logger::Env;
use ethercrab::{
    MainDevice, MainDeviceConfig, PduStorage, SubDeviceGroup, Timeouts, error::Error,
    std::ethercat_now,
};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::time::MissedTickBehavior;

/// Maximum number of SubDevices that can be stored. This must be a power of 2 greater than 1.
const MAX_SUBDEVICES: usize = 16;
/// Maximum PDU data payload size - set this to the max PDI size or higher.
const MAX_PDU_DATA: usize = PduStorage::element_size(1100);
/// Maximum number of EtherCAT frames that can be in flight at any one time.
const MAX_FRAMES: usize = 16;

static PDU_STORAGE: PduStorage<MAX_FRAMES, MAX_PDU_DATA> = PduStorage::new();

#[derive(Default)]
struct Groups {
    /// EL2889 and EK1100/EK1501. For EK1100, 2 items, 2 bytes of PDI for 16 output bits. The EK1501
    /// has 2 bytes of its own PDI so we'll use an upper bound of 4.
    ///
    /// We'll keep the EK1100/EK1501 in here as it has no useful PDI but still needs to live
    /// somewhere.
    slow_outputs: SubDeviceGroup<2, 4>,
    /// EL2828. 1 item, 1 byte of PDI for 8 output bits.
    fast_outputs: SubDeviceGroup<1, 1>,
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    let interface = std::env::args()
        .nth(1)
        .expect("Provide network interface as first argument.");

    log::info!("Starting multiple groups demo...");
    log::info!(
        "Ensure an EK1100 or EK1501 is the first SubDevice, with an EL2828 and EL2889 following it"
    );
    log::info!("Run with RUST_LOG=ethercrab=debug or =trace for debug information");

    let (tx, rx, pdu_loop) = PDU_STORAGE.try_split().expect("can only split once");

    let maindevice = MainDevice::new(
        pdu_loop,
        Timeouts {
            wait_loop_delay: Duration::from_millis(2),
            mailbox_response: Duration::from_millis(1000),
            ..Default::default()
        },
        MainDeviceConfig::default(),
    );

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
    // Network TX/RX should run in a separate thread to avoid timeouts. Tokio doesn't guarantee a
    // separate thread is used but this is good enough for an example. If using `tokio`, make sure
    // the `rt-multi-thread` feature is enabled.
    #[cfg(not(target_os = "windows"))]
    tokio::spawn(ethercrab::std::tx_rx_task(&interface, tx, rx).expect("spawn TX/RX task"));

    let maindevice = Arc::new(maindevice);

    // Read configurations from SubDevice EEPROMs and configure devices.
    let groups = maindevice
        .init::<MAX_SUBDEVICES, _>(ethercat_now, |groups: &Groups, subdevice| {
            match subdevice.name() {
                "EL2889" | "EK1100" | "EK1501" => Ok(&groups.slow_outputs),
                "EL2828" => Ok(&groups.fast_outputs),
                _ => Err(Error::UnknownSubDevice),
            }
        })
        .await
        .expect("Init");

    let Groups {
        slow_outputs,
        fast_outputs,
    } = groups;

    let maindevice_slow = maindevice.clone();

    let slow_task = tokio::spawn(async move {
        let slow_outputs = slow_outputs
            .into_op(&maindevice_slow)
            .await
            .expect("PRE-OP -> OP");

        let mut slow_cycle_time = tokio::time::interval(Duration::from_millis(3));
        slow_cycle_time.set_missed_tick_behavior(MissedTickBehavior::Skip);

        let slow_duration = Duration::from_millis(250);

        // Only update "slow" outputs every 250ms using this instant
        let mut tick = Instant::now();

        // EK1100 is first SubDevice, EL2889 is second
        let el2889 = slow_outputs
            .subdevice(&maindevice_slow, 1)
            .expect("EL2889 not present!");

        // Set initial output state
        el2889.outputs_raw_mut()[0] = 0x01;
        el2889.outputs_raw_mut()[1] = 0x80;

        loop {
            let Ok(_) = slow_outputs.tx_rx(&maindevice_slow).await else {
                break;
            };

            // Increment every output byte for every SubDevice by one
            if tick.elapsed() > slow_duration {
                tick = Instant::now();

                let el2889 = slow_outputs
                    .subdevice(&maindevice_slow, 1)
                    .expect("EL2889 not present!");

                let mut o = el2889.outputs_raw_mut();

                // Make a nice pattern on EL2889 LEDs
                o[0] = o[0].rotate_left(1);
                o[1] = o[1].rotate_right(1);
            }

            slow_cycle_time.tick().await;
        }
    });

    let fast_task = tokio::spawn(async move {
        let fast_outputs = fast_outputs
            .into_op(&maindevice)
            .await
            .expect("PRE-OP -> OP");

        let mut fast_cycle_time = tokio::time::interval(Duration::from_millis(5));
        fast_cycle_time.set_missed_tick_behavior(MissedTickBehavior::Skip);

        loop {
            let Ok(_) = fast_outputs.tx_rx(&maindevice).await else {
                break;
            };

            // Increment every output byte for every SubDevice by one
            for subdevice in fast_outputs.iter(&maindevice) {
                let mut o = subdevice.outputs_raw_mut();

                for byte in o.iter_mut() {
                    *byte = byte.wrapping_add(1);
                }
            }

            fast_cycle_time.tick().await;
        }
    });

    let (slow, fast) = tokio::join!(slow_task, fast_task);

    slow.expect("slow task failed");
    fast.expect("fast task failed");

    Ok(())
}
