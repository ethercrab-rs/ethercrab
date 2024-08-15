//! Test that SDO multi writes behave as expected.
//!
//! - EK1914
//! - EL3004

mod util;

use env_logger::Env;
use ethercrab::{error::Error, MainDevice, MainDeviceConfig, PduStorage, Timeouts};
use std::path::PathBuf;

const MAX_SUBDEVICES: usize = 16;
const MAX_PDU_DATA: usize = PduStorage::element_size(1100);
const MAX_FRAMES: usize = 128;
const PDI_LEN: usize = 128;

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn replay_ek1914_el3004_configure() -> Result<(), Error> {
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
    assert_eq!(group.subdevice(&maindevice, 1)?.name(), "EL3004");

    let el3004 = group.subdevice(&maindevice, 1)?;

    el3004.sdo_write(0x1c12, 0, 0u8).await?;

    el3004
        .sdo_write_subindices(0x1c13, &[0x1a00u16, 0x1a02, 0x1a04, 0x1a06])
        .await?;

    assert_eq!(el3004.sdo_read::<u16>(0x1c13, 1).await?, 0x1a00u16);
    assert_eq!(el3004.sdo_read::<u16>(0x1c13, 2).await?, 0x1a02u16);
    assert_eq!(el3004.sdo_read::<u16>(0x1c13, 3).await?, 0x1a04u16);
    assert_eq!(el3004.sdo_read::<u16>(0x1c13, 4).await?, 0x1a06u16);
    assert_eq!(el3004.sdo_read::<u8>(0x1c13, 0).await?, 4u8);

    Ok(())
}
