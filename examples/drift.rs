//! Discover devices connected to the network.

use env_logger::Env;
use ethercrab::{std::tx_rx_task, Client, ClientConfig, PduStorage, RegisterAddress, Timeouts};
use futures_lite::StreamExt;
use rustix::time::ClockId;
use std::{sync::Arc, time::Duration};

/// Maximum number of slaves that can be stored. This must be a power of 2 greater than 1.
const MAX_SLAVES: usize = 128;
/// Maximum PDU data payload size - set this to the max PDI size or higher.
const MAX_PDU_DATA: usize = 1100;
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

    let client = Arc::new(Client::new(
        pdu_loop,
        Timeouts::default(),
        ClientConfig {
            dc_static_sync_iterations: 10_000,
            ..ClientConfig::default()
        },
    ));

    smol::block_on(async {
        smol::spawn(tx_rx_task(&interface, tx, rx).expect("spawn TX/RX task")).detach();

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

        let mut tick = smol::Timer::interval(Duration::from_millis(1000));

        while let Some(_) = tick.next().await {
            let t_ns = {
                let t = rustix::time::clock_gettime(ClockId::Realtime);

                (t.tv_sec * 1000 * 1000) + t.tv_nsec
            };

            log::info!("Master time {} ns", t_ns);

            for s in group.iter(&client) {
                log::info!(
                    "--> {:#06x} system time {} ns, offset {} ns, xmit delay {} ns, diff {} ns",
                    s.configured_address(),
                    s.register_read::<u64>(RegisterAddress::DcSystemTime)
                        .await
                        .unwrap_or(0),
                    s.register_read::<u64>(RegisterAddress::DcSystemTimeOffset)
                        .await
                        .unwrap_or(0),
                    s.register_read::<u32>(RegisterAddress::DcSystemTimeTransmissionDelay)
                        .await
                        .unwrap_or(0),
                    s.register_read::<u32>(RegisterAddress::DcSystemTimeDifference)
                        .await
                        .unwrap_or(0),
                );
            }
        }
    });

    log::info!("Done.");
}
