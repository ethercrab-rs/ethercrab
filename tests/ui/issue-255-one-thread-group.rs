//! From [#255](https://github.com/ethercrab-rs/ethercrab/issues/255#issue-2735565244). Reduced from
//! `examples/ek1100.rs`.

use ethercrab::{
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

    let group = group.into_op(&maindevice).await.expect("PRE-OP -> OP");

    let mut tick_interval = tokio::time::interval(Duration::from_millis(5));
    tick_interval.set_missed_tick_behavior(MissedTickBehavior::Skip);

    let group = std::sync::Arc::new(group);

    tokio::spawn({
        let maindevice = maindevice.clone();
        let group = group.clone();

        async move {
            loop {
                group.tx_rx(&maindevice).await.expect("TX/RX");
            }
        }
    });

    loop {
        {
            let mut subdevice = group.subdevice(&maindevice, 0).unwrap();
            let (_i, o) = subdevice.io_raw_mut();

            for byte in o.iter_mut() {
                *byte = byte.wrapping_add(1);
            }
        }

        tick_interval.tick().await;
    }

    #[allow(unreachable_code)]
    Ok(())
}
