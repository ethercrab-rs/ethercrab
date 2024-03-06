//! Discover devices connected to the network.

use env_logger::Env;
use ethercrab::{std::tx_rx_task, Client, ClientConfig, PduStorage, RegisterAddress, Timeouts};
use futures_lite::StreamExt;
use rustix::time::ClockId;
use std::{path::PathBuf, sync::Arc, time::Duration};

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

    let mut csv_path = PathBuf::from(
        std::env::args()
            .nth(2)
            .expect("Provide CSV output path as second argument."),
    );

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
            .init_single_group::<MAX_SLAVES, PDI_LEN>(|| {
                let rustix::fs::Timespec { tv_sec, tv_nsec } =
                    rustix::time::clock_gettime(rustix::time::ClockId::Monotonic);

                let t = (tv_sec * 1000 * 1000 * 1000 + tv_nsec) as u64;

                // EtherCAT epoch is 2000-01-01
                t.saturating_sub(946684800)
            })
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

        let mut tick = smol::Timer::interval(Duration::from_millis(200));

        csv_path.set_extension("csv");

        let mut wtr = csv::Writer::from_path(&csv_path).expect("Unable to create writer");

        #[derive(serde::Serialize)]
        struct CsvRow {
            name: String,
            cycle: usize,
            slave: u16,
            system_time: u64,
            system_time_offset: i64,
            xmit_delay: u32,
            diff: i32,
        }

        let mut cycle = 0;

        for _ in 0..200 {
            let t_ns = {
                let t = rustix::time::clock_gettime(ClockId::Realtime);

                (t.tv_sec * 1000 * 1000) + t.tv_nsec
            };

            // log::info!("Master time {} ns", t_ns);

            for s in group.iter(&client) {
                let system_time = s
                    .register_read::<u64>(RegisterAddress::DcSystemTime)
                    .await
                    .unwrap_or(0);
                let system_time_offset = s
                    .register_read::<i64>(RegisterAddress::DcSystemTimeOffset)
                    .await
                    .unwrap_or(0);
                let xmit_delay = s
                    .register_read::<u32>(RegisterAddress::DcSystemTimeTransmissionDelay)
                    .await
                    .unwrap_or(0);
                let diff = s
                    .register_read::<u32>(RegisterAddress::DcSystemTimeDifference)
                    .await
                    .map(|raw: u32| {
                        let greater_than = (raw & (1u32 << 31)) == 0;

                        let value = (raw & (u32::MAX >> 1)) as i32;

                        if greater_than {
                            value
                        } else {
                            -value
                        }
                    })
                    .unwrap_or(0);

                wtr.serialize(CsvRow {
                    name: csv_path.file_stem().unwrap().to_string_lossy().to_string(),
                    cycle,
                    slave: s.configured_address(),
                    system_time,
                    system_time_offset,
                    xmit_delay,
                    diff,
                })
                .expect("CSV write");

                wtr.serialize(CsvRow {
                    name: csv_path.file_stem().unwrap().to_string_lossy().to_string(),
                    cycle,
                    slave: 0x0000,
                    system_time: t_ns as u64,
                    system_time_offset: 0,
                    xmit_delay: 0,
                    diff: 0,
                })
                .expect("CSV write");

                // log::info!(
                //     "--> {:#06x} system time {} ns, offset {} ns, xmit delay {} ns, diff {} ns",
                //     s.configured_address(),
                //     system_time,
                //     system_time_offset,
                //     xmit_delay,
                //     diff,
                // );
            }

            cycle += 1;

            tick.next().await;
        }
    });

    log::info!("Done.");
}
