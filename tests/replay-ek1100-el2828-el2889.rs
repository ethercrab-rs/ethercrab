//! Replay comms between EK1100, EL2828, EL2889. Based on `multiple-groups` demo at time of writing.
//!
//! Required hardware:
//!
//! - EK1100
//! - EL2828
//! - EL2889

mod util;

use env_logger::Env;
use ethercrab::{
    error::Error, subdevice_group, MainDevice, MainDeviceConfig, PduStorage, SubDeviceGroup,
    Timeouts,
};
use std::{path::PathBuf, time::Duration};
use tokio::time::MissedTickBehavior;

const MAX_SUBDEVICES: usize = 16;
const MAX_PDU_DATA: usize = PduStorage::element_size(1100);
const MAX_FRAMES: usize = 128;

#[derive(Default)]
struct Groups {
    /// EL2889 and EK1100. 2 items, 2 bytes of PDI for 16 output bits.
    ///
    /// We'll keep the EK1100 in here as it has no PDI but still needs to live somewhere.
    slow_outputs: SubDeviceGroup<2, 2, subdevice_group::PreOp>,
    /// EL2828. 1 item, 1 byte of PDI for 8 output bits.
    fast_outputs: SubDeviceGroup<1, 1, subdevice_group::PreOp>,
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn replay_ek1100_el2828_el2889() -> Result<(), Error> {
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
    let groups = maindevice
        .init::<MAX_SUBDEVICES, _>(
            || 0,
            |groups: &Groups, subdevice| match subdevice.name() {
                "EL2889" | "EK1100" => Ok(&groups.slow_outputs),
                "EL2828" => Ok(&groups.fast_outputs),
                _ => Err(Error::UnknownSubDevice),
            },
        )
        .await
        .expect("Init");

    let Groups {
        slow_outputs,
        fast_outputs,
    } = groups;

    let slow_outputs = slow_outputs
        .into_op(&maindevice)
        .await
        .expect("Slow into OP");
    let fast_outputs = fast_outputs
        .into_op(&maindevice)
        .await
        .expect("Fast into OP");

    let mut slow_cycle_time = tokio::time::interval(Duration::from_millis(10));
    slow_cycle_time.set_missed_tick_behavior(MissedTickBehavior::Skip);

    {
        let mut el2889 = slow_outputs
            .subdevice(&maindevice, 1)
            .expect("EL2889 not present!");

        // Set initial output state
        el2889.io_raw_mut().1[0] = 0x01;
        el2889.io_raw_mut().1[1] = 0x80;
    }

    // Animate slow pattern for 8 ticks
    for _ in 0..8 {
        slow_outputs.tx_rx(&maindevice).await.expect("TX/RX");

        let mut el2889 = slow_outputs
            .subdevice(&maindevice, 1)
            .expect("EL2889 not present!");

        let (_i, o) = el2889.io_raw_mut();

        // Make a nice pattern on EL2889 LEDs
        o[0] = o[0].rotate_left(1);
        o[1] = o[1].rotate_right(1);

        slow_cycle_time.tick().await;
    }

    let mut fast_cycle_time = tokio::time::interval(Duration::from_millis(5));
    fast_cycle_time.set_missed_tick_behavior(MissedTickBehavior::Skip);

    // Count up to 255 in binary
    for _ in 0..255 {
        fast_outputs.tx_rx(&maindevice).await.expect("TX/RX");

        // Increment every output byte for every SubDevice by one
        for mut subdevice in fast_outputs.iter(&maindevice) {
            let (_i, o) = subdevice.io_raw_mut();

            for byte in o.iter_mut() {
                *byte = byte.wrapping_add(1);
            }
        }

        fast_cycle_time.tick().await;
    }

    Ok(())
}
