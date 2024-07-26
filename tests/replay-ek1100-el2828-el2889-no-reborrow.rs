//! Borrowing different SubDevices at the same time is ok, but borrowing the same one more than once is
//! not.
//!
//! Required hardware:
//!
//! - EK1100
//! - EL2828
//! - EL2889

mod util;

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
    slow_outputs: SubDeviceGroup<2, 2, subdevice_group::PreOp>,
    fast_outputs: SubDeviceGroup<1, 1, subdevice_group::PreOp>,
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn replay_ek1100_el2828_el2889_no_reborrow() -> Result<(), Error> {
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

    let _el2828 = fast_outputs
        .subdevice(&maindevice, 0)
        .expect("EL2828 not present!");
    let _ek1100 = slow_outputs
        .subdevice(&maindevice, 0)
        .expect("EL2889 not present!");
    let _el2889 = slow_outputs
        .subdevice(&maindevice, 1)
        .expect("EL2889 not present!");

    let el2889_2 = slow_outputs.subdevice(&maindevice, 1);

    assert!(matches!(el2889_2, Err(Error::Borrow)));

    Ok(())
}
