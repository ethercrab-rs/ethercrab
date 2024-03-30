//! Test that mailboxes can be read/written. This test requires:
//!
//! - EK1914
//! - EL3004

mod util;

use env_logger::Env;
use ethercrab::{error::Error, Client, ClientConfig, PduStorage, Timeouts};
use std::path::PathBuf;

const MAX_SLAVES: usize = 16;
const MAX_PDU_DATA: usize = PduStorage::element_size(1100);
const MAX_FRAMES: usize = 128;
const PDI_LEN: usize = 128;

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn replay_ek1914_el3004_mailbox() -> Result<(), Error> {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

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

    let test_name = PathBuf::from(file!())
        .file_stem()
        .unwrap()
        .to_string_lossy()
        .to_string();

    util::spawn_tx_rx(&format!("tests/{test_name}.pcapng"), tx, rx);

    // Read configurations from slave EEPROMs and configure devices.
    let mut group = client
        .init_single_group::<MAX_SLAVES, PDI_LEN>(|| 0)
        .await
        .expect("Init");

    assert_eq!(group.slave(&client, 0)?.name(), "EK1914");
    assert_eq!(group.slave(&client, 1)?.name(), "EL3004");

    let mut configured = false;

    for slave in group.iter(&client) {
        log::info!("--> Slave {}", slave.name());

        if slave.name() == "EL3004" {
            log::info!("--> Configuring EL3004");

            // Check we can read
            let fw_version = slave.sdo_read::<heapless::String<32>>(0x100a, 0).await?;

            log::info!("----> FW version: {}", fw_version);

            // Check we can write
            slave.sdo_write(0xf008, 0, 1u32).await?;

            log::info!("----> Wrote outputs");

            assert_eq!(
                slave.sdo_read(0xf008, 0).await,
                Ok(1u32),
                "written value was not stored"
            );

            configured = true;
        }
    }

    assert!(configured, "did not find target slave");

    Ok(())
}
