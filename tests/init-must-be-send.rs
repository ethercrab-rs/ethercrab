//! A weird looking test, but it just makes sure the EtherCrab init routines are `Send`.

use core::future::Future;
use ethercrab::{MainDevice, MainDeviceConfig, PduStorage, Timeouts, std::ethercat_now};
use std::{sync::Arc, time::Duration};

#[test]
fn init_must_be_send() {
    fn spawn<'a, F, T>(_fut: F) -> Result<(), ()>
    where
        F: Future<Output = T> + Send + 'a,
        T: 'a,
    {
        // Don't bother running the future - this is just a compile test
        Ok(())
    }

    let _ = spawn(init());
}

const MAX_SUBDEVICES: usize = 16;
const MAX_PDU_DATA: usize = 1100;
const MAX_FRAMES: usize = 16;
const PDI_LEN: usize = 64;

static PDU_STORAGE: PduStorage<MAX_FRAMES, MAX_PDU_DATA> = PduStorage::new();

async fn init() {
    let (_tx, _rx, pdu_loop) = PDU_STORAGE.try_split().expect("can only split once");

    let maindevice = Arc::new(MainDevice::new(
        pdu_loop,
        Timeouts {
            wait_loop_delay: Duration::from_millis(2),
            mailbox_response: Duration::from_millis(1000),
            ..Default::default()
        },
        MainDeviceConfig::default(),
    ));

    let _group = maindevice
        .init_single_group::<MAX_SUBDEVICES, PDI_LEN>(ethercat_now)
        .await
        .expect("Init");
}
