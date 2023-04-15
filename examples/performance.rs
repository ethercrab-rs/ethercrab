//! Demonstrate running the TX/RX loop in a separate realtime thread.
//!
//! Please note that this example is currently quite unstable on my (@jamwaffles) test system so
//! YMMV!

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("This example is only supported on Linux systems");
}

#[cfg(target_os = "linux")]
#[tokio::main]
async fn main() -> Result<(), ethercrab::error::Error> {
    use env_logger::Env;
    use ethercrab::{
        error::Error, std::tx_rx_task, Client, ClientConfig, PduStorage, SlaveGroup,
        SlaveGroupContainer, SlaveGroupRef, Timeouts,
    };
    use rustix::process::CpuSet;
    use smol::LocalExecutor;
    use std::{
        sync::Arc,
        time::{Duration, Instant},
    };
    use thread_priority::{
        RealtimeThreadSchedulePolicy, ThreadPriority, ThreadPriorityValue, ThreadSchedulePolicy,
    };
    use tokio::time::MissedTickBehavior;

    /// Maximum number of slaves that can be stored.
    const MAX_SLAVES: usize = 16;
    /// Maximum PDU data payload size - set this to the max PDI size or higher.
    const MAX_PDU_DATA: usize = 1100;
    /// Maximum number of EtherCAT frames that can be in flight at any one time.
    const MAX_FRAMES: usize = 16;

    static PDU_STORAGE: PduStorage<MAX_FRAMES, MAX_PDU_DATA> = PduStorage::new();

    #[derive(Default)]
    struct Groups {
        /// EL2889 and EK1100. 2 items, 2 bytes of PDI for 16 output bits.
        ///
        /// We'll keep the EK1100 in here as it has no PDI but still needs to live somewhere.
        slow_outputs: SlaveGroup<2, 2>,
        /// EL2828. 1 item, 1 byte of PDI for 8 output bits.
        fast_outputs: SlaveGroup<1, 1>,
    }

    impl SlaveGroupContainer for Groups {
        fn num_groups(&self) -> usize {
            2
        }

        fn group(&mut self, index: usize) -> Option<SlaveGroupRef> {
            match index {
                0 => Some(self.slow_outputs.as_mut()),
                1 => Some(self.fast_outputs.as_mut()),
                _ => None,
            }
        }
    }

    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    let interface = std::env::args()
        .nth(1)
       .expect("Provide interface as first argument. Pass an unrecognised name to list available interfaces.");

    log::info!("Starting multiple groups demo...");
    log::info!(
        "Ensure an EK1100 is the first slave device, with an EL2828 and EL2889 following it"
    );
    log::info!("Run with RUST_LOG=ethercrab=debug or =trace for debug information");

    let (tx, rx, pdu_loop) = PDU_STORAGE.try_split().expect("can only split once");

    let client = Client::new(
        pdu_loop,
        Timeouts {
            wait_loop_delay: Duration::from_millis(2),
            mailbox_response: Duration::from_millis(1000),
            ..Default::default()
        },
        ClientConfig::default(),
    );

    thread_priority::ThreadBuilder::default()
        .name("tx-rx-task")
        // Might need to set `<user> hard rtprio 99` and `<user> soft rtprio 99` in `/etc/security/limits.conf`
        // Check limits with `ulimit -Hr` or `ulimit -Sr`
        .priority(ThreadPriority::Crossplatform(
            ThreadPriorityValue::try_from(99u8).unwrap(),
        ))
        // NOTE: Requires a realtime kernel
        .policy(ThreadSchedulePolicy::Realtime(
            RealtimeThreadSchedulePolicy::Fifo,
        ))
        .spawn(move |_| {
            let mut set = CpuSet::new();
            set.set(0);

            // Pin thread to 0th core
            rustix::process::sched_setaffinity(None, &set).expect("set affinity");

            let ex = LocalExecutor::new();

            futures_lite::future::block_on(
                ex.run(tx_rx_task(&interface, tx, rx).expect("spawn TX/RX task")),
            )
            .expect("TX/RX task exited");
        })
        .unwrap();

    let client = Arc::new(client);

    // Read configurations from slave EEPROMs and configure devices.
    let groups = client
        .init::<MAX_SLAVES, _>(Groups::default(), |groups, slave| {
            match slave.name.as_str() {
                "EL2889" | "EK1100" => Ok(groups.slow_outputs.as_mut()),
                "EL2828" => Ok(groups.fast_outputs.as_mut()),
                _ => Err(Error::UnknownSlave),
            }
        })
        .await
        .expect("Init");

    let Groups {
        slow_outputs,
        fast_outputs,
    } = groups;

    let client_slow = client.clone();

    let slow_task = tokio::spawn(async move {
        let mut slow_cycle_time = tokio::time::interval(Duration::from_millis(3));
        slow_cycle_time.set_missed_tick_behavior(MissedTickBehavior::Skip);

        let slow_duration = Duration::from_millis(250);

        // Only update "slow" outputs every 250ms using this instant
        let mut tick = Instant::now();

        // EK1100 is first slave, EL2889 is second
        let el2889 = slow_outputs.slave(1).expect("EL2889 not present!");

        // Set initial output state
        el2889.io().1[0] = 0x01;
        el2889.io().1[1] = 0x80;

        loop {
            slow_outputs.tx_rx(&client_slow).await.expect("TX/RX");

            // Increment every output byte for every slave device by one
            if tick.elapsed() > slow_duration {
                tick = Instant::now();

                let (_i, o) = el2889.io();

                // Make a nice pattern on EL2889 LEDs
                o[0] = o[0].rotate_left(1);
                o[1] = o[1].rotate_right(1);
            }

            slow_cycle_time.tick().await;
        }
    });

    let fast_task = tokio::spawn(async move {
        let mut fast_cycle_time = tokio::time::interval(Duration::from_millis(5));
        fast_cycle_time.set_missed_tick_behavior(MissedTickBehavior::Skip);

        loop {
            fast_outputs.tx_rx(&client).await.expect("TX/RX");

            // Increment every output byte for every slave device by one
            for slave in fast_outputs.slaves() {
                let (_i, o) = slave.io();

                for byte in o.iter_mut() {
                    *byte = byte.wrapping_add(1);
                }
            }

            fast_cycle_time.tick().await;
        }
    });

    let (slow, fast) = tokio::join!(slow_task, fast_task);

    slow.expect("slow task failed");
    fast.expect("fast task failed");

    Ok(())
}
