use ethercrab::{MainDevice, MainDeviceConfig, PduLoop, Timeouts};
use std::{sync::Arc, time::Duration};

pub struct EthercatMaster<'a> {
    pdu_loop: Option<PduLoop<'a>>,
    maindevice: Option<Arc<MainDevice<'a>>>,
}

impl<'a> EthercatMaster<'a> {
    pub fn new(pdu_loop: PduLoop<'a>) -> Self {
        let maindevice = Some(Arc::new(MainDevice::new(
            pdu_loop,
            Timeouts {
                wait_loop_delay: Duration::ZERO,
                state_transition: Duration::from_secs(50),
                pdu: Duration::from_millis(2000),
                eeprom: Duration::from_millis(50),
                ..Timeouts::default()
            },
            MainDeviceConfig {
                dc_static_sync_iterations: 10_000,
                ..MainDeviceConfig::default()
            },
        )));

        Self {
            maindevice,
            pdu_loop: None,
        }
    }

    pub fn release(&mut self) {
        log::info!("Going to release all resources of Ethercat Master!");

        // Release main device
        let maindevice = self
            .maindevice
            .take()
            .and_then(Arc::into_inner)
            .expect("MainDevice should not be held at this point");

        // SAFETY: Any groups created with the current `maindevice` MUST be dropped before this line.
        // They cannot be reused with a new `MainDevice` instance and must be initialised again.
        self.pdu_loop = Some(unsafe { maindevice.release_all() });
    }

    pub fn close(&mut self) {
        self.release();
    }

    pub fn init(&mut self) {
        self.maindevice = Some(Arc::new(MainDevice::new(
            self.pdu_loop.take().unwrap(),
            Timeouts {
                wait_loop_delay: Duration::ZERO,
                state_transition: Duration::from_secs(50),
                pdu: Duration::from_millis(2000),
                eeprom: Duration::from_millis(50),
                ..Timeouts::default()
            },
            MainDeviceConfig {
                dc_static_sync_iterations: 10_000,
                ..MainDeviceConfig::default()
            },
        )));
    }
}

fn main() {}
