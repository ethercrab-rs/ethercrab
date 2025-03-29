//! Dump the EEPROM of a given sub device to stdout.

use env_logger::Env;
use ethercrab::{
    MainDevice, MainDeviceConfig, PduStorage, Timeouts, error::Error, std::ethercat_now,
};
use std::io::Write;

/// Maximum number of SubDevices that can be stored. This must be a power of 2 greater than 1.
const MAX_SUBDEVICES: usize = 16;
/// Maximum PDU data payload size - set this to the max PDI size or higher.
const MAX_PDU_DATA: usize = PduStorage::element_size(1100);
/// Maximum number of EtherCAT frames that can be in flight at any one time.
const MAX_FRAMES: usize = 16;
/// Maximum total PDI length.
const PDI_LEN: usize = 64;

static PDU_STORAGE: PduStorage<MAX_FRAMES, MAX_PDU_DATA> = PduStorage::new();

#[tokio::main]
async fn main() -> Result<(), Error> {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    let interface = std::env::args()
        .nth(1)
        .expect("Provide network interface as first argument.");

    let index: u16 = std::env::args()
        .nth(2)
        .expect("Provide device index (starting from zero) as second argument.")
        .parse()
        .expect("Invalid index: must be a number");

    log::info!(
        "Starting EEPROM dump tool, interface {}, device index {}",
        interface,
        index
    );

    let (tx, rx, pdu_loop) = PDU_STORAGE.try_split().expect("can only split once");

    let maindevice = MainDevice::new(
        pdu_loop,
        Timeouts::default(),
        MainDeviceConfig {
            dc_static_sync_iterations: 0,
            ..MainDeviceConfig::default()
        },
    );

    #[cfg(target_os = "windows")]
    std::thread::spawn(move || {
        ethercrab::std::tx_rx_task_blocking(
            &interface,
            tx,
            rx,
            ethercrab::std::TxRxTaskConfig { spinloop: false },
        )
        .expect("TX/RX task")
    });
    #[cfg(not(target_os = "windows"))]
    tokio::spawn(ethercrab::std::tx_rx_task(&interface, tx, rx).expect("spawn TX/RX task"));

    let group = maindevice
        .init_single_group::<MAX_SUBDEVICES, PDI_LEN>(ethercat_now)
        .await
        .expect("Init");

    log::info!("Discovered {} SubDevices", group.len());

    for subdevice in group.iter(&maindevice) {
        log::info!(
            "--> SubDevices {:#06x} {} {}",
            subdevice.configured_address(),
            subdevice.name(),
            subdevice.identity()
        );
    }

    let subdevice = group
        .subdevice(&maindevice, usize::from(index))
        .expect("Could not find device for given index");

    log::info!(
        "Dumping EEPROM for device index {}: {:#06x} {} {} {}...",
        index,
        subdevice.configured_address(),
        subdevice.name(),
        subdevice
            .description()
            .await?
            .map(|d| d.as_str().to_string())
            .unwrap_or(String::new()),
        subdevice.identity()
    );

    let eeprom_len = subdevice
        .eeprom_size(&maindevice)
        .await
        .expect("Could not read EEPROM len");

    log::info!("--> Device EEPROM is {} bytes long", eeprom_len);

    let mut buf = vec![0u8; eeprom_len];

    // Read entire EEPROM into buffer
    subdevice.eeprom_read_raw(&maindevice, 0, &mut buf).await?;

    std::io::stdout().write_all(&buf[..]).expect("Stdout write");

    log::info!("Done, wrote {} bytes to stdout", buf.len());

    Ok(())
}
