//! From [#255](https://github.com/ethercrab-rs/ethercrab/issues/255#issuecomment-2539273742). Reduced from
//! `examples/ec400.rs`.

use ethercrab::{
    ds402::{Ds402, Ds402Sm},
    error::Error,
    std::{ethercat_now, tx_rx_task},
    MainDevice, MainDeviceConfig, PduStorage, Timeouts,
};
use std::{sync::Arc, time::Duration};
use tokio::time::MissedTickBehavior;

const MAX_SUBDEVICES: usize = 16;
const MAX_PDU_DATA: usize = PduStorage::element_size(1100);
const MAX_FRAMES: usize = 16;
const PDI_LEN: usize = 64;

static PDU_STORAGE: PduStorage<MAX_FRAMES, MAX_PDU_DATA> = PduStorage::new();

#[tokio::main]
async fn main() -> Result<(), Error> {
    let interface = "eth0";

    let (tx, rx, pdu_loop) = PDU_STORAGE.try_split().expect("can only split once");

    let mut maindevice = Arc::new(MainDevice::new(
        pdu_loop,
        Timeouts::default(),
        MainDeviceConfig::default(),
    ));

    tokio::spawn(tx_rx_task(&interface, tx, rx).expect("spawn TX/RX task"));

    let group = Arc::get_mut(&mut maindevice)
        .unwrap()
        .init_single_group::<MAX_SUBDEVICES, PDI_LEN>(ethercat_now)
        .await
        .expect("Init");

    let mut group = group.into_op(&maindevice).await.expect("PRE-OP -> OP");

    // Run twice to prime PDI
    group.tx_rx(&maindevice).await.expect("TX/RX");

    // Read cycle time from servo drive
    let cycle_time = {
        let subdevice = group.subdevice(&maindevice, 0).unwrap();

        let base = subdevice.sdo_read::<u8>(0x60c2, 1).await?;
        let x10 = subdevice.sdo_read::<i8>(0x60c2, 2).await?;

        let base = f32::from(base);
        let x10 = 10.0f32.powi(i32::from(x10));

        let cycle_time_ms = (base * x10) * 1000.0;

        Duration::from_millis(unsafe { cycle_time_ms.round().to_int_unchecked() })
    };

    let mut cyclic_interval = tokio::time::interval(cycle_time);
    cyclic_interval.set_missed_tick_behavior(MissedTickBehavior::Skip);

    let subdevice = group.subdevice(&maindevice, 0).expect("No servo!");
    let mut servo = Ds402Sm::new(Ds402::new(subdevice).expect("Failed to gather DS402"));

    loop {
        group.tx_rx(&maindevice).await.expect("TX/RX");

        let (i, o) = servo.subdevice().io_raw_mut();

        dbg!(i, o);

        cyclic_interval.tick().await;
    }

    #[allow(unreachable_code)]
    Ok(())
}
