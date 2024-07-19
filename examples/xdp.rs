//! TODO: Document this example

use env_logger::Env;
use ethercrab::{
    error::Error,
    std::{ethercat_now, tx_rx_task_xdp},
    MainDevice, MainDeviceConfig, PduStorage, Timeouts,
};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use thread_priority::{
    RealtimeThreadSchedulePolicy, ThreadPriority, ThreadPriorityValue, ThreadSchedulePolicy,
};
use tokio::time::MissedTickBehavior;

/// Maximum number of slaves that can be stored. This must be a power of 2 greater than 1.
const MAX_SLAVES: usize = 16;
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
        "Ensure an EK1100 or EK1501 is the first slave, with any number of modules connected after"
    );
    log::info!("Run with RUST_LOG=ethercrab=debug or =trace for debug information");

    let (tx, rx, pdu_loop) = PDU_STORAGE.try_split().expect("can only split once");

    let client = Arc::new(Client::new(
        pdu_loop,
        Timeouts {
            wait_loop_delay: Duration::from_millis(2),
            mailbox_response: Duration::from_millis(1000),
            pdu: Duration::from_millis(2_000),
            ..Default::default()
        },
        ClientConfig::default(),
    ));

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
            // core_affinity::set_for_current(tx_rx_core)
            //     .then_some(())
            //     .expect("Set TX/RX thread core");

            // Blocking io_uring
            tx_rx_task_xdp(&interface, tx, rx).expect("TX/RX task");
        })
        .unwrap();

    let mut group = client
        .init_single_group::<MAX_SLAVES, PDI_LEN>(ethercat_now)
        .await
        .expect("Init");

    log::info!("Discovered {} slaves", group.len());

    for slave in group.iter(&client) {
        if slave.name() == "EL3004" {
            log::info!("Found EL3004. Configuring...");

            slave.sdo_write(0x1c12, 0, 0u8).await?;
            slave.sdo_write(0x1c13, 0, 0u8).await?;

            slave.sdo_write(0x1c13, 1, 0x1a00u16).await?;
            slave.sdo_write(0x1c13, 2, 0x1a02u16).await?;
            slave.sdo_write(0x1c13, 3, 0x1a04u16).await?;
            slave.sdo_write(0x1c13, 4, 0x1a06u16).await?;
            slave.sdo_write(0x1c13, 0, 4u8).await?;
        }
    }

    let mut group = group.into_op(&client).await.expect("PRE-OP -> OP");

    for slave in group.iter(&client) {
        let (i, o) = slave.io_raw();

        log::info!(
            "-> Slave {:#06x} {} inputs: {} bytes, outputs: {} bytes",
            slave.configured_address(),
            slave.name(),
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

        group.tx_rx(&client).await.expect("TX/RX");

        // Increment every output byte for every slave device by one
        for mut slave in group.iter(&client) {
            let (_i, o) = slave.io_raw_mut();

            for byte in o.iter_mut() {
                *byte = byte.wrapping_add(1);
            }
        }

        tick_interval.tick().await;
    }

    let group = group.into_safe_op(&client).await.expect("OP -> SAFE-OP");

    log::info!("OP -> SAFE-OP");

    let group = group.into_pre_op(&client).await.expect("SAFE-OP -> PRE-OP");

    log::info!("SAFE-OP -> PRE-OP");

    let _group = group.into_init(&client).await.expect("PRE-OP -> INIT");

    log::info!("PRE-OP -> INIT, shutdown complete");

    Ok(())
}
