use ethercrab::{std::ethercat_now, MainDevice, MainDeviceConfig, PduStorage, Timeouts};
use std::thread;
use std::{sync::Arc, time::Duration};

const MAX_SUBDEVICES: usize = 16;
const MAX_PDU_DATA: usize = 1100;
const MAX_FRAMES: usize = 16;
const PDI_LEN: usize = 64;

static PDU_STORAGE: PduStorage<MAX_FRAMES, MAX_PDU_DATA> = PduStorage::new();

fn main() {
    let (_tx, _rx, pdu_loop) = PDU_STORAGE.try_split().expect("can only split once");

    let mut maindevice = Arc::new(MainDevice::new(
        pdu_loop,
        Timeouts {
            wait_loop_delay: Duration::from_millis(2),
            mailbox_response: Duration::from_millis(1000),
            ..Default::default()
        },
        MainDeviceConfig::default(),
    ));

    // We should not be able to concurrently init!
    thread::scope(|s| {
        s.spawn(|| {
            let _group = smol::block_on(
                Arc::get_mut(&mut maindevice)
                    .unwrap()
                    .init_single_group::<MAX_SUBDEVICES, PDI_LEN>(ethercat_now),
            )
            .expect("Init");
        });

        s.spawn(|| {
            let _group = smol::block_on(
                Arc::get_mut(&mut maindevice)
                    .unwrap()
                    .init_single_group::<MAX_SUBDEVICES, PDI_LEN>(ethercat_now),
            )
            .expect("Init");
        });
    });
}
