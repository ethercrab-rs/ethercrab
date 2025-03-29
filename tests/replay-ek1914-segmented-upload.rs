//! Test that segmented CoE uploads work.
//!
//! - EK1914

mod util;

use env_logger::Env;
use ethercrab::{MainDevice, MainDeviceConfig, PduStorage, Timeouts, error::Error};
use std::path::PathBuf;

const MAX_SUBDEVICES: usize = 16;
const MAX_PDU_DATA: usize = PduStorage::element_size(1100);
const MAX_FRAMES: usize = 128;
const PDI_LEN: usize = 128;

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn replay_ek1914_segmented_upload() -> Result<(), Error> {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    static PDU_STORAGE: PduStorage<MAX_FRAMES, MAX_PDU_DATA> = PduStorage::new();

    let (tx, rx, pdu_loop) = PDU_STORAGE.try_split().expect("can only split once");

    let maindevice = MainDevice::new(
        pdu_loop,
        Timeouts::default(),
        MainDeviceConfig {
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

    // Read configurations from SubDevice EEPROMs and configure devices.
    let group = maindevice
        .init_single_group::<MAX_SUBDEVICES, PDI_LEN>(|| 0)
        .await
        .expect("Init");

    assert_eq!(group.subdevice(&maindevice, 0)?.name(), "EK1914");

    let first = group
        .subdevice(&maindevice, 0)
        .expect("EK1914 must be first");

    let name_coe = first
        .sdo_read::<heapless::String<32>>(0x1008, 0)
        .await
        .expect("Failed to read name");

    log::info!("Device name: {:?}", name_coe);

    assert_eq!(&name_coe, "EK1914");

    Ok(())
}
