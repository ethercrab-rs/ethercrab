//! Checking that CoE abort codes are reported correctly.
//!
//! - EK1914

mod util;

use ethercrab::{
    error::{CoeAbortCode, Error, MailboxError},
    Client, ClientConfig, PduStorage, RetryBehaviour, SlaveGroupState, Timeouts,
};
use std::time::Duration;

const MAX_SLAVES: usize = 16;
const MAX_PDU_DATA: usize = 1100;
const MAX_FRAMES: usize = 128;
const PDI_LEN: usize = 128;

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn replay_sdo_error() -> Result<(), Error> {
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
        .init_single_group::<MAX_SLAVES, PDI_LEN>()
        .await
        .expect("Init");

    let first = group.slave(&client, 0)?;

    assert_eq!(
        first.name(),
        "EK1914",
        "Only EK1914 is supported for this case"
    );

    assert_eq!(
        first.sdo_read::<u8>(0x1001, 0).await,
        Err(Error::Mailbox(MailboxError::Aborted {
            code: CoeAbortCode::NotFound,
            address: 0x1001,
            sub_index: 0.into(),
        }))
    );

    assert_eq!(
        first.sdo_read::<u8>(0x1008, 0).await,
        Err(Error::Mailbox(MailboxError::TooLong {
            address: 0x1008,
            sub_index: 0.into(),
        }))
    );

    Ok(())
}
