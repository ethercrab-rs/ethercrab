//! Demonstrates the simplest way to create an EtherCAT master with a single group and no PREOP ->
//! SAFEOP hook function.

use env_logger::Env;
use ethercrab::{
    error::Error, std::tx_rx_task, Client, ClientConfig, PduStorage, SlaveGroup, SlaveState,
    Timeouts,
};
use std::{sync::Arc, time::Duration};
use tokio::time::MissedTickBehavior;

/// Maximum number of slaves that can be stored. This must be a power of 2 greater than 1.
const MAX_SLAVES: usize = 16;
/// Maximum PDU data payload size - set this to the max PDI size or higher.
const MAX_PDU_DATA: usize = 1100;
/// Maximum number of EtherCAT frames that can be in flight at any one time.
const MAX_FRAMES: usize = 16;
/// Reserve 8 bytes for PDI.
const PDI_LEN: usize = 8;

static PDU_STORAGE: PduStorage<MAX_FRAMES, MAX_PDU_DATA> = PduStorage::new();

#[tokio::main]
async fn main() -> Result<(), Error> {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    let interface = std::env::args()
        .nth(1)
        .expect("Provide network interface as first argument.");

    let (tx, rx, pdu_loop) = PDU_STORAGE.try_split().expect("can only split once");

    let client = Client::new(pdu_loop, Timeouts::default(), ClientConfig::default());

    // Network TX/RX should run in a separate thread to avoid timeouts. Tokio doesn't guarantee a
    // separate thread is used but this is good enough for an example. If using `tokio`, make sure
    // the `rt-multi-thread` feature is enabled.
    tokio::spawn(tx_rx_task(&interface, tx, rx).expect("spawn TX/RX task"));

    let client = Arc::new(client);

    let mut group = client
        // `SlaveGroup::new()` can be used instead of `SlaveGroup::default()` if a PREOP -> SAFEOP
        // hook function is required.
        .init_single_group::<MAX_SLAVES, PDI_LEN>(SlaveGroup::default())
        .await
        .expect("Init");

    client
        .request_slave_state(SlaveState::Op)
        .await
        .expect("OP");

    let mut cycle_time = tokio::time::interval(Duration::from_millis(5));
    cycle_time.set_missed_tick_behavior(MissedTickBehavior::Skip);

    loop {
        group.tx_rx(&client).await.expect("TX/RX");

        // Increment every output byte for every slave device by one
        for slave in group.iter(&client) {
            let (_i, o) = slave.io_raw();

            for byte in o.iter_mut() {
                *byte = byte.wrapping_add(1);
            }
        }

        cycle_time.tick().await;
    }
}
