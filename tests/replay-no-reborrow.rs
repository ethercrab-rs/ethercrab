//! Borrowing different slaves at the same time is ok, but borrowing the same one more than once is
//! not.
//!
//! Required hardware:
//!
//! - EK1100
//! - EL2828
//! - EL2889

mod util;

use ethercrab::{
    error::Error, slave_group, Client, ClientConfig, PduStorage, SlaveGroup, SlaveGroupState,
    Timeouts,
};
use std::time::Duration;
use tokio::time::MissedTickBehavior;

const MAX_SLAVES: usize = 16;
const MAX_PDU_DATA: usize = PduStorage::element_size(1100);
const MAX_FRAMES: usize = 128;

#[derive(Default)]
struct Groups {
    slow_outputs: SlaveGroup<2, 2, slave_group::PreOp>,
    fast_outputs: SlaveGroup<1, 1, slave_group::PreOp>,
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn replay_no_reborrow() -> Result<(), Error> {
    static PDU_STORAGE: PduStorage<MAX_FRAMES, MAX_PDU_DATA> = PduStorage::new();

    let (tx, rx, pdu_loop) = PDU_STORAGE.try_split().expect("can only split once");

    let client = Client::new(
        pdu_loop,
        Timeouts::default(),
        ClientConfig {
            dc_static_sync_iterations: 100,
            ..Default::default()
        },
    );

    util::spawn_tx_rx("tests/replay-no-reborrow.pcapng", tx, rx);

    // Read configurations from slave EEPROMs and configure devices.
    let groups = client
        .init::<MAX_SLAVES, _>(
            || 0,
            |groups: &Groups, slave| match slave.name() {
                "EL2889" | "EK1100" => Ok(&groups.slow_outputs),
                "EL2828" => Ok(&groups.fast_outputs),
                _ => Err(Error::UnknownSlave),
            },
        )
        .await
        .expect("Init");

    let Groups {
        slow_outputs,
        fast_outputs,
    } = groups;

    let slow_outputs = slow_outputs.into_op(&client).await.expect("Slow into OP");
    let fast_outputs = fast_outputs.into_op(&client).await.expect("Fast into OP");

    let mut slow_cycle_time = tokio::time::interval(Duration::from_millis(10));
    slow_cycle_time.set_missed_tick_behavior(MissedTickBehavior::Skip);

    let _el2828 = fast_outputs.slave(&client, 0).expect("EL2828 not present!");
    let _ek1100 = slow_outputs.slave(&client, 0).expect("EL2889 not present!");
    let _el2889 = slow_outputs.slave(&client, 1).expect("EL2889 not present!");

    let el2889_2 = slow_outputs.slave(&client, 1);

    assert!(matches!(el2889_2, Err(Error::Borrow)));

    Ok(())
}
