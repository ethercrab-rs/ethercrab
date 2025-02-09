//! Set alias address.
//!
//! Required hardware:
//!
//! - EK1100

mod util;

use env_logger::Env;
use ethercrab::{error::Error, MainDevice, MainDeviceConfig, PduStorage, Timeouts};
use std::{path::PathBuf, time::Duration};
use tokio::time::sleep;

const MAX_SUBDEVICES: usize = 16;
const MAX_PDU_DATA: usize = PduStorage::element_size(1100);
const MAX_FRAMES: usize = 128;
const PDI_LEN: usize = 64;

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn replay_ek1100_alias_address() -> Result<(), Error> {
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

    log::debug!("Beginning init");

    let mut group = maindevice
        .init_single_group::<MAX_SUBDEVICES, PDI_LEN>(|| 0)
        .await
        .expect("Init");

    log::debug!("Init complete");

    let mut sds = group.iter_mut(&maindevice);

    let mut ek1100 = sds.next().expect("at least one subdevice required");

    assert_eq!(ek1100.name(), "EK1100", "Group must start with EK1100");

    ek1100.set_alias_address(0xabcd).await?;

    log::debug!("Alias set");

    // Let EEPROM settle a bit
    sleep(Duration::from_millis(50)).await;

    assert_eq!(
        ek1100.read_alias_address_from_eeprom(&maindevice).await,
        Ok(0xabcd)
    );

    sleep(Duration::from_millis(50)).await;

    // Reset
    ek1100.set_alias_address(0x0000).await?;

    sleep(Duration::from_millis(50)).await;

    assert_eq!(
        ek1100.read_alias_address_from_eeprom(&maindevice).await,
        Ok(0x0000)
    );

    sleep(Duration::from_millis(50)).await;

    Ok(())
}
