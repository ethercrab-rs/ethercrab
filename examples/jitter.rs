//! Jitter measurement. Requires PREEMPT-RT kernel. Run with `--release` for best results.

use env_logger::Env;
use ethercrab::{error::Error, std::tx_rx_task, Client, ClientConfig, PduStorage, Timeouts};
use futures_lite::StreamExt;
use std::{
    sync::Arc,
    time::{Duration, Instant},
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

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("This example is only supported on Linux systems");
}

#[cfg(target_os = "linux")]
fn main() -> Result<(), Error> {
    use ethercrab::std::ethercat_now;
    use thread_priority::{
        set_thread_priority_and_policy, thread_native_id, RealtimeThreadSchedulePolicy,
        ThreadPriority, ThreadPriorityValue, ThreadSchedulePolicy,
    };

    // These values (99/FIFO) require a realtime kernel. Tested with PREEMPT_RT but others may work
    // too.
    let thread_id = thread_native_id();
    set_thread_priority_and_policy(
        thread_id,
        ThreadPriority::Crossplatform(ThreadPriorityValue::try_from(99u8).unwrap()),
        ThreadSchedulePolicy::Realtime(RealtimeThreadSchedulePolicy::Fifo),
    )
    .expect("could not set thread priority. Are the PREEMPT_RT patches in use?");

    smol::block_on(async {
        env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

        let interface = std::env::args()
            .nth(1)
            .expect("Provide network interface as first argument.");

        log::info!("Starting EK1100 demo...");
        log::info!(
            "Ensure an EK1100 is the first slave, with any number of modules connected after"
        );
        log::info!("Run with RUST_LOG=ethercrab=debug or =trace for debug information");

        let (tx, rx, pdu_loop) = PDU_STORAGE.try_split().expect("can only split once");

        let client = Arc::new(Client::new(
            pdu_loop,
            Timeouts {
                wait_loop_delay: Duration::from_millis(2),
                mailbox_response: Duration::from_millis(1000),
                ..Default::default()
            },
            ClientConfig::default(),
        ));

        smol::spawn(tx_rx_task(&interface, tx, rx).expect("spawn TX/RX task")).detach();

        let group = client
            .init_single_group::<MAX_SLAVES, PDI_LEN>(ethercat_now)
            .await
            .expect("Init");

        log::info!("Discovered {} slaves", group.len());

        let mut group = group.into_op(&client).await.expect("OP");

        for slave in group.iter(&client) {
            let (i, o) = slave.io_raw();

            log::info!(
                "-> Slave {:#06x} {} inputs: {} bytes, outputs: {} bytes",
                slave.configured_address(),
                slave.name(),
                i.len(),
                o.len()
            );
        }

        let period = Duration::from_millis(1);

        let mut smol_timer = smol::Timer::interval(period);

        let (tx, rx) = smol::channel::bounded(5);

        smol::spawn(async move {
            let mut histo = hdrhistogram::Histogram::<u64>::new(3).expect("Histogram");

            let mut now = Instant::now();

            let clear_at = Duration::from_secs(10);
            let start = Instant::now();
            let mut cleared = false;

            println!("Warming up...");

            let mut max_sd = 0.0f64;

            while let Ok(record) = rx.recv().await {
                histo.record(record).expect("record");

                // Clear warmup data
                if start.elapsed() >= clear_at && !cleared {
                    histo.clear();

                    cleared = true;

                    continue;
                }

                if now.elapsed() > Duration::from_secs(1) && start.elapsed() > clear_at {
                    now = Instant::now();

                    let sd = histo.stdev().round() / period.as_nanos() as f64 * 100.0;

                    max_sd = max_sd.max(sd);

                    println!(
                        "{}s: mean {:.3} ms, std dev {:.3} ms ({:3.2} % / {:3.2} % max)",
                        start.elapsed().as_secs(),
                        histo.mean() / 1000.0 / 1000.0,
                        histo.stdev() / 1000.0 / 1000.0,
                        sd,
                        max_sd
                    );
                }
            }
        })
        .detach();

        loop {
            let prev_time = Instant::now();

            group.tx_rx(&client).await.expect("TX/RX");

            // Increment every output byte for every slave device by one
            for mut slave in group.iter(&client) {
                let (_i, o) = slave.io_raw_mut();

                for byte in o.iter_mut() {
                    *byte = byte.wrapping_add(1);
                }
            }

            smol_timer.next().await;

            let tick_time = prev_time.elapsed();

            tx.send(tick_time.as_nanos() as u64).await.ok();
        }
    });

    Ok(())
}
