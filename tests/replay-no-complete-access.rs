//! Checking that slave devices with no support for CoE complete access still initialise.
//!
//! Required hardware:
//!
//! - EK1914 (does not support CoE complete access)

mod util;

use ethercrab::{error::Error, Client, ClientConfig, PduStorage, RetryBehaviour, Timeouts};
use std::time::Duration;

const MAX_SLAVES: usize = 16;
const MAX_PDU_DATA: usize = 1100;
const MAX_FRAMES: usize = 128;
const PDI_LEN: usize = 128;

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn replay_no_complete_access() -> Result<(), Error> {
    static PDU_STORAGE: PduStorage<MAX_FRAMES, MAX_PDU_DATA> = PduStorage::new();

    let (tx, rx, pdu_loop) = PDU_STORAGE.try_split().expect("can only split once");

    let client = Client::new(
        pdu_loop,
        Timeouts {
            wait_loop_delay: Duration::from_millis(5),
            ..Timeouts::default()
        },
        ClientConfig {
            dc_static_sync_iterations: 100,
            retry_behaviour: RetryBehaviour::None,
            ..Default::default()
        },
    );

    util::spawn_tx_rx("tests/replay-no-complete-access.pcapng", tx, rx);

    // Read configurations from slave EEPROMs and configure devices.
    let group = client
        .init_single_group::<MAX_SLAVES, PDI_LEN>(|| {
            let rustix::fs::Timespec { tv_sec, tv_nsec } =
                rustix::time::clock_gettime(rustix::time::ClockId::Monotonic);

            let t = (tv_sec * 1000 * 1000 * 1000 + tv_nsec) as u64;

            // EtherCAT epoch is 2000-01-01
            t.saturating_sub(946684800)
        })
        .await
        .expect("Init");

    let _group = group.into_op(&client).await.expect("PRE-OP -> OP");

    Ok(())
}
