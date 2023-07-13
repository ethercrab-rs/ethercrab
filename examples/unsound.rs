//! DELETEME: This is just to check some unsound fixes

use env_logger::Env;
use ethercrab::{
    error::Error, std::tx_rx_task, Client, ClientConfig, PduStorage, SlaveGroup, SlaveState,
    Timeouts,
};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::time::MissedTickBehavior;

/// Maximum number of slaves that can be stored. This must be a power of 2 greater than 1.
const MAX_SLAVES: usize = 16;
/// Maximum PDU data payload size - set this to the max PDI size or higher.
const MAX_PDU_DATA: usize = 1100;
/// Maximum number of EtherCAT frames that can be in flight at any one time.
const MAX_FRAMES: usize = 16;

static PDU_STORAGE: PduStorage<MAX_FRAMES, MAX_PDU_DATA> = PduStorage::new();

#[derive(Default)]
struct Groups {
    /// EL2889 and EK1100. 2 items, 2 bytes of PDI for 16 output bits.
    ///
    /// We'll keep the EK1100 in here as it has no PDI but still needs to live somewhere.
    slow_outputs: SlaveGroup<2, 2>,
    /// EL2828. 1 item, 1 byte of PDI for 8 output bits.
    fast_outputs: SlaveGroup<1, 1>,
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    let interface = std::env::args()
        .nth(1)
        .expect("Provide network interface as first argument.");

    log::info!("Starting multiple groups demo...");
    log::info!(
        "Ensure an EK1100 is the first slave device, with an EL2828 and EL2889 following it"
    );
    log::info!("Run with RUST_LOG=ethercrab=debug or =trace for debug information");

    let (tx, rx, pdu_loop) = PDU_STORAGE.try_split().expect("can only split once");

    let client = Client::new(
        pdu_loop,
        Timeouts {
            wait_loop_delay: Duration::from_millis(2),
            mailbox_response: Duration::from_millis(1000),
            ..Default::default()
        },
        ClientConfig::default(),
    );

    // Network TX/RX should run in a separate thread to avoid timeouts. Tokio doesn't guarantee a
    // separate thread is used but this is good enough for an example. If using `tokio`, make sure
    // the `rt-multi-thread` feature is enabled.
    tokio::spawn(tx_rx_task(&interface, tx, rx).expect("spawn TX/RX task"));

    let client = Arc::new(client);

    // Read configurations from slave EEPROMs and configure devices.
    let groups = client
        .init::<MAX_SLAVES, _>(Groups::default(), |groups, slave| match slave.name() {
            "EL2889" | "EK1100" => Ok(&groups.slow_outputs),
            "EL2828" => Ok(&groups.fast_outputs),
            _ => Err(Error::UnknownSlave),
        })
        .await
        .expect("Init");

    let Groups {
        slow_outputs,
        mut fast_outputs,
    } = groups;

    let client_slow = client.clone();

    client
        .request_slave_state(SlaveState::Op)
        .await
        .expect("OP");

    let slow_task: tokio::task::JoinHandle<Result<_, Error>> = tokio::spawn(async move {
        let mut slow_cycle_time = tokio::time::interval(Duration::from_millis(3));
        slow_cycle_time.set_missed_tick_behavior(MissedTickBehavior::Skip);

        let slow_duration = Duration::from_millis(250);

        // Only update "slow" outputs every 250ms using this instant
        let mut tick = Instant::now();

        // EK1100 is first slave, EL2889 is second
        let el2889 = slow_outputs.slave(&client_slow, 1)?;

        // FIXME: This shouldn't be possible
        let el2889_bad = slow_outputs.slave(&client_slow, 1)?;

        // Set initial output state
        el2889.io_raw().1[0] = 0x01;
        el2889.io_raw().1[1] = 0x80;

        loop {
            slow_outputs.tx_rx(&client_slow).await.expect("TX/RX");

            // Increment every output byte for every slave device by one
            if tick.elapsed() > slow_duration {
                tick = Instant::now();

                let (_i, o) = el2889.io_raw();

                // Make a nice pattern on EL2889 LEDs
                o[0] = o[0].rotate_left(1);
                o[1] = o[1].rotate_right(1);
            }

            slow_cycle_time.tick().await;
        }

        #[allow(unreachable_code)]
        Ok(())
    });

    let fast_task: tokio::task::JoinHandle<Result<_, Error>> = tokio::spawn(async move {
        let mut fast_cycle_time = tokio::time::interval(Duration::from_millis(5));
        fast_cycle_time.set_missed_tick_behavior(MissedTickBehavior::Skip);

        loop {
            fast_outputs.tx_rx(&client).await.expect("TX/RX");

            // Increment every output byte for every slave device by one
            for slave in fast_outputs.iter(&client) {
                // fast_outputs.iter(&client).for_each(|_| {
                //     // FIXME: This is bad!
                // });

                // // FIXME: This is also bad
                // let sl = fast_outputs.slave(&client, 0).unwrap();

                let (_i, o) = slave.io_raw();

                for byte in o.iter_mut() {
                    *byte = byte.wrapping_add(1);
                }
            }

            // This is ok because we're outside the iterator
            let _sl = fast_outputs.slave(&client, 0)?;

            fast_cycle_time.tick().await;
        }

        #[allow(unreachable_code)]
        Ok(())
    });

    let (slow, fast) = tokio::try_join!(slow_task, fast_task).unwrap();

    slow.expect("slow task failed");
    fast.expect("fast task failed");

    Ok(())
}
