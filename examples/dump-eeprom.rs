//! Dump the EEPROM of a given sub device to stdout.
//!
//! Requires the unstable `__internals` feature to be enabled.

use std::io::Write;

use embedded_io_async::Read;
use env_logger::Env;
use ethercrab::{
    error::Error,
    internals::{EepromDataProvider, SiiDataProvider, SlaveClient},
    std::tx_rx_task,
    Client, ClientConfig, PduStorage, SlaveGroupState, Timeouts,
};

/// Maximum number of slaves that can be stored. This must be a power of 2 greater than 1.
const MAX_SLAVES: usize = 16;
/// Maximum PDU data payload size - set this to the max PDI size or higher.
const MAX_PDU_DATA: usize = 1100;
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

    let client = Client::new(pdu_loop, Timeouts::default(), ClientConfig::default());

    tokio::spawn(tx_rx_task(&interface, tx, rx).expect("spawn TX/RX task"));

    let mut group = client
        .init_single_group::<MAX_SLAVES, PDI_LEN>()
        .await
        .expect("Init");

    log::info!("Discovered {} slaves", group.len());

    for slave in group.iter(&client) {
        log::info!(
            "--> Slave {:#06x} {} {}",
            slave.configured_address(),
            slave.name(),
            slave.identity()
        );
    }

    let slave = group
        .slave(&client, usize::from(index))
        .expect("Could not find device for given index");

    log::info!(
        "Dumping EEPROM for device index {}: {:#06x} {} {}...",
        index,
        slave.configured_address(),
        slave.name(),
        slave.identity()
    );

    let base_address = 0x1000;

    let slave_client = SlaveClient::new(&client, base_address + index);

    let provider = SiiDataProvider::new(&slave_client);

    let len = provider.len().await.expect("Len");

    log::info!("--> Device EEPROM is {} bytes long", len);

    let mut reader = provider.reader();

    let mut buf = vec![0u8; usize::from(len)];

    reader.read_exact(&mut buf).await.expect("Read exact");

    std::io::stdout().write_all(&buf[..]).expect("Stdout write");

    log::info!("Done, wrote {} bytes", buf.len());

    Ok(())
}