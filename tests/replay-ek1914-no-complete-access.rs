//! Checking that SubDevices with no support for CoE complete access still initialise.
//!
//! Required hardware:
//!
//! - EK1914 (does not support CoE complete access)

mod util;

use ethercrab::{error::Error, MainDevice, MainDeviceConfig, PduStorage, RetryBehaviour, Timeouts};
use std::{path::PathBuf, time::Duration};

const MAX_SUBDEVICES: usize = 16;
const MAX_PDU_DATA: usize = PduStorage::element_size(1100);
const MAX_FRAMES: usize = 128;
const PDI_LEN: usize = 128;

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn replay_ek1914_no_complete_access() -> Result<(), Error> {
    static PDU_STORAGE: PduStorage<MAX_FRAMES, MAX_PDU_DATA> = PduStorage::new();

    let (tx, rx, pdu_loop) = PDU_STORAGE.try_split().expect("can only split once");

    let maindevice = MainDevice::new(
        pdu_loop,
        Timeouts {
            wait_loop_delay: Duration::from_millis(5),
            ..Timeouts::default()
        },
        MainDeviceConfig {
            dc_static_sync_iterations: 100,
            retry_behaviour: RetryBehaviour::None,
        },
    );

    let test_name = PathBuf::from(file!())
        .file_stem()
        .unwrap()
        .to_string_lossy()
        .to_string();

    util::spawn_tx_rx(&format!("tests/{test_name}.pcapng"), tx, rx);

    // Read configurations from SubDevice EEPROMs and configure devices.
    let group = maindevice
        .init_single_group::<MAX_SUBDEVICES, PDI_LEN>(|| 0)
        .await
        .expect("Init");

    assert_eq!(group.subdevice(&maindevice, 0)?.name(), "EK1914");

    let _group = group.into_op(&maindevice).await.expect("PRE-OP -> OP");

    Ok(())
}
