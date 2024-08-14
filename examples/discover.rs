//! Discover devices connected to the network.

use env_logger::Env;
use ethercrab::{
    std::{ethercat_now, tx_rx_task},
    MainDevice, MainDeviceConfig, PduStorage, Timeouts,
};
use std::{str::FromStr, sync::Arc};

/// Maximum number of SubDevices that can be stored. This must be a power of 2 greater than 1.
const MAX_SUBDEVICES: usize = 128;
/// Maximum PDU data payload size - set this to the max PDI size or higher.
const MAX_PDU_DATA: usize = PduStorage::element_size(1100);
/// Maximum number of EtherCAT frames that can be in flight at any one time.
const MAX_FRAMES: usize = 16;
/// Maximum total PDI length.
const PDI_LEN: usize = 64;

static PDU_STORAGE: PduStorage<MAX_FRAMES, MAX_PDU_DATA> = PduStorage::new();

fn main() {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    let interface = std::env::args()
        .nth(1)
        .expect("Provide network interface as first argument.");

    log::info!("Discovering EtherCAT devices on {}...", interface);

    let (tx, rx, pdu_loop) = PDU_STORAGE.try_split().expect("can only split once");

    let maindevice = Arc::new(MainDevice::new(
        pdu_loop,
        Timeouts::default(),
        MainDeviceConfig {
            dc_static_sync_iterations: 0,
            ..MainDeviceConfig::default()
        },
    ));

    smol::block_on(async {
        smol::spawn(tx_rx_task(&interface, tx, rx).expect("spawn TX/RX task")).detach();

        let mut group = maindevice
            .init_single_group::<MAX_SUBDEVICES, PDI_LEN>(ethercat_now)
            .await
            .expect("Init");

        log::info!("Discovered {} SubDevices", group.len());

        for subdevice in group.iter(&maindevice) {
            log::info!(
                "--> SubDevice {:#06x} name {}, description {}, {}",
                subdevice.configured_address(),
                subdevice.name(),
                subdevice
                    .description()
                    .await
                    .expect("Failed to read description")
                    .unwrap_or(heapless::String::<64>::from_str("[no description]").unwrap()),
                subdevice.identity()
            );
        }
    });

    log::info!("Done.");
}
