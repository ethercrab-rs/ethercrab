//! Test that mailboxes can be read/written. This test requires:
//!
//! - EK1100
//! - EL6910

mod util;

use env_logger::Env;
use ethercrab::{error::Error, Client, ClientConfig, PduStorage, Timeouts};

const MAX_SLAVES: usize = 16;
const MAX_PDU_DATA: usize = 1100;
const MAX_FRAMES: usize = 128;
const PDI_LEN: usize = 128;

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn replay_mailbox_el6910() -> Result<(), Error> {
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

    util::spawn_tx_rx("tests/replay-mailbox-el6910.pcapng", tx, rx);

    // Read configurations from slave EEPROMs and configure devices.
    let mut group = client
        .init_single_group::<MAX_SLAVES, PDI_LEN>(|| {
            let rustix::fs::Timespec { tv_sec, tv_nsec } =
                rustix::time::clock_gettime(rustix::time::ClockId::Monotonic);

            let t = (tv_sec * 1000 * 1000 * 1000 + tv_nsec) as u64;

            // EtherCAT epoch is 2000-01-01
            t.saturating_sub(946684800)
        })
        .await
        .expect("Init");

    let mut configured = false;

    for slave in group.iter(&client) {
        log::info!("--> Slave {}", slave.name());

        if slave.name() == "EL6910" {
            log::info!("--> Configuring EL6910");

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

    assert_eq!(configured, true, "did not find target slave");

    Ok(())
}
