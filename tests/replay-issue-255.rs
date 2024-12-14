//! Capture network traffic for `issue-255.rs`
//!
//! Required hardware:
//!
//! - EK1100
//! - EL2828

mod util;

use env_logger::Env;
use ethercrab::{error::Error, MainDevice, MainDeviceConfig, PduStorage, Timeouts};
use std::{hint::black_box, path::PathBuf, time::Duration};
use tokio::time::MissedTickBehavior;

const MAX_SUBDEVICES: usize = 16;
const MAX_PDU_DATA: usize = PduStorage::element_size(1100);
const MAX_FRAMES: usize = 128;
const MAX_PDI: usize = 128;

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn replay_issue_255() -> Result<(), Error> {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    static PDU_STORAGE: PduStorage<MAX_FRAMES, MAX_PDU_DATA> = PduStorage::new();

    let (tx, rx, pdu_loop) = PDU_STORAGE.try_split().expect("can only split once");

    let mut maindevice = MainDevice::new(
        pdu_loop,
        Timeouts::default(),
        MainDeviceConfig {
            dc_static_sync_iterations: 0,
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
        .init_single_group::<MAX_SUBDEVICES, MAX_PDI>(time_zero)
        .await
        .expect("Init");

    let mut group = group.into_op(&maindevice).await.expect("Slow into OP");

    let mut cycle_time = tokio::time::interval(Duration::from_millis(2));
    cycle_time.set_missed_tick_behavior(MissedTickBehavior::Skip);

    // Animate slow pattern for 8 ticks
    for _ in 0..64 {
        group.tx_rx(&maindevice).await.expect("TX/RX");

        let mut el2889 = group.subdevice(&maindevice, 1).unwrap();

        let (_i, o) = el2889.io_raw_mut();

        black_box(do_stuff(black_box(o)));

        cycle_time.tick().await;
    }

    Ok(())
}

fn time_zero() -> u64 {
    0
}

fn do_stuff(slice: &mut [u8]) {
    slice[0] += 1;
    slice[0] -= 1;
}
