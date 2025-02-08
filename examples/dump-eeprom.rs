//! Dump the EEPROM of a given sub device to stdout.
//!
//! Requires the unstable `__internals` feature to be enabled.

use std::io::Write;

use embedded_io_async::Read;
use env_logger::Env;
use ethercrab::{
    error::Error,
    internals::{DeviceEeprom, EepromRange},
    std::{ethercat_now, tx_rx_task},
    MainDevice, MainDeviceConfig, PduStorage, Timeouts,
};

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

    tokio::spawn(tx_rx_task(&interface, tx, rx).expect("spawn TX/RX task"));

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
        "Dumping EEPROM for device index {}: {:#06x} {} {}...",
        index,
        subdevice.configured_address(),
        subdevice.name(),
        subdevice.identity()
    );

    let base_address = 0x1000;

    let mut len_buf = [0u8; 2];

    // ETG2020 page 7: 0x003e is the EEPROM address size register in kilobit minus 1 (u16).
    EepromRange::new(
        DeviceEeprom::new(&maindevice, base_address + index),
        0x003e,
        2,
    )
    .read_exact(&mut len_buf)
    .await
    .expect("Could not read EEPROM len");

    // Kilobits to bits to bytes, and undoing the offset
    let len = ((u16::from_le_bytes(len_buf) + 1) * 1024) / 8;

    log::info!("--> Device EEPROM is {} bytes long", len);

    let mut provider =
        EepromRange::new(DeviceEeprom::new(&maindevice, base_address + index), 0, len);

    let mut buf = vec![0u8; usize::from(len)];

    provider.read_exact(&mut buf).await.expect("Read");

    std::io::stdout().write_all(&buf[..]).expect("Stdout write");

    log::info!("Done, wrote {} bytes to stdout", buf.len());

    Ok(())
}
