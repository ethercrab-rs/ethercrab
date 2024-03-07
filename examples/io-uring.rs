//! Use blocking io_uring-based TX/RX loop.
//!
//! This example pins the TX/RX loop to core 0, and two other 100us tasks to cores 1 and 2.
//!
//! You may need to increase `INTERVAL` as 100us can be challenging for some PCs. That said, a
//! Raspberry Pi 4 with a realtime kernel and some tweaking can run 2x 100us tasks _ok_.
//!
//! This example requires a Linux with `io_uring` support and a realtime kernel (e.g. `PREEMPT_RT`).

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("This example is only supported on Linux systems");
}

#[cfg(target_os = "linux")]
fn main() -> Result<(), ethercrab::error::Error> {
    use env_logger::{Env, TimestampPrecision};
    use ethercrab::{
        error::Error,
        std::{ethercat_now, tx_rx_task_io_uring},
        Client, ClientConfig, PduStorage, SlaveGroup, SlaveGroupState, Timeouts,
    };
    use std::{
        sync::Arc,
        time::{Duration, Instant},
    };
    use thread_priority::{
        RealtimeThreadSchedulePolicy, ThreadPriority, ThreadPriorityValue, ThreadSchedulePolicy,
    };
    use timerfd::{SetTimeFlags, TimerFd, TimerState};

    /// Maximum number of slaves that can be stored. This must be a power of 2 greater than 1.
    const MAX_SLAVES: usize = 16;
    /// Maximum PDU data payload size - set this to the max PDI size or higher.
    const MAX_PDU_DATA: usize = 1100;
    /// Maximum number of EtherCAT frames that can be in flight at any one time.
    const MAX_FRAMES: usize = 16;
    /// Interval in microseconds.
    const INTERVAL: u64 = 100;

    static PDU_STORAGE: PduStorage<MAX_FRAMES, MAX_PDU_DATA> = PduStorage::new();

    #[derive(Default)]
    struct Groups {
        /// EL2889 and EK1100/EK1501. For EK1100, 2 items, 2 bytes of PDI for 16 output bits. The EK1501
        /// has 2 bytes of its own PDI so we'll use an upper bound of 4.
        ///
        /// We'll keep the EK1100/EK1501 in here as it has no useful PDI but still needs to live
        /// somewhere.
        slow_outputs: SlaveGroup<2, 4>,
        /// EL2828. 1 item, 1 byte of PDI for 8 output bits.
        fast_outputs: SlaveGroup<1, 1>,
    }

    env_logger::Builder::from_env(Env::default().default_filter_or("info"))
        .format_timestamp(Some(TimestampPrecision::Nanos))
        .init();

    let interface = std::env::args()
        .nth(1)
        .expect("Provide network interface as first argument.");

    log::info!("Starting multiple groups demo...");
    log::info!(
        "Ensure an EK1100 or EK1501 is the first slave device, with an EL2828 and EL2889 following it"
    );
    log::info!("Run with RUST_LOG=ethercrab=debug or =trace for debug information");

    let (tx, rx, pdu_loop) = PDU_STORAGE.try_split().expect("can only split once");

    let core_ids = core_affinity::get_core_ids().expect("Couldn't get core IDs");

    let tx_rx_core = core_ids
        .get(0)
        .copied()
        .expect("At least one core is required. Are you running on a potato?");
    let slow_core = core_ids
        .get(1)
        .copied()
        .expect("At least 2 cores are required.");
    let fast_core = core_ids
        .get(2)
        .copied()
        .expect("At least 3 cores are required.");

    thread_priority::ThreadBuilder::default()
        .name("tx-rx-thread")
        // Might need to set `<user> hard rtprio 99` and `<user> soft rtprio 99` in `/etc/security/limits.conf`
        // Check limits with `ulimit -Hr` or `ulimit -Sr`
        .priority(ThreadPriority::Crossplatform(
            ThreadPriorityValue::try_from(49u8).unwrap(),
        ))
        // NOTE: Requires a realtime kernel
        .policy(ThreadSchedulePolicy::Realtime(
            RealtimeThreadSchedulePolicy::Fifo,
        ))
        .spawn(move |_| {
            core_affinity::set_for_current(tx_rx_core)
                .then_some(())
                .expect("Set TX/RX thread core");

            // Blocking io_uring
            tx_rx_task_io_uring(&interface, tx, rx).expect("TX/RX task");
        })
        .unwrap();

    let client = Client::new(pdu_loop, Timeouts::default(), ClientConfig::default());

    let client = Arc::new(client);

    // Read configurations from slave EEPROMs and configure devices.
    let Groups {
        slow_outputs,
        fast_outputs,
    } = futures_lite::future::block_on(client.init::<MAX_SLAVES, _>(
        |groups: &Groups, slave| match slave.name() {
            "EL2889" | "EK1100" | "EK1501" => Ok(&groups.slow_outputs),
            "EL2828" => Ok(&groups.fast_outputs),
            _ => Err(Error::UnknownSlave),
        },
        ethercat_now,
    ))
    .expect("Init");

    let client_slow = client.clone();

    let slow = thread_priority::ThreadBuilder::default()
        .name("slow-task")
        // Might need to set `<user> hard rtprio 99` and `<user> soft rtprio 99` in `/etc/security/limits.conf`
        // Check limits with `ulimit -Hr` or `ulimit -Sr`
        .priority(ThreadPriority::Crossplatform(
            ThreadPriorityValue::try_from(48u8).unwrap(),
        ))
        // NOTE: Requires a realtime kernel
        .policy(ThreadSchedulePolicy::Realtime(
            RealtimeThreadSchedulePolicy::Fifo,
        ))
        .spawn(move |_| {
            core_affinity::set_for_current(slow_core)
                .then_some(())
                .expect("Set slow thread core");

            futures_lite::future::block_on::<Result<(), Error>>(async {
                let slow_outputs = slow_outputs
                    .into_op(&client_slow)
                    .await
                    .expect("PRE-OP -> OP");

                let slow_cycle_time = Duration::from_micros(INTERVAL);

                let mut tfd = TimerFd::new().unwrap();

                tfd.set_state(
                    TimerState::Periodic {
                        current: slow_cycle_time,
                        interval: slow_cycle_time,
                    },
                    SetTimeFlags::Default,
                );

                let slow_duration = Duration::from_millis(250);

                // Only update "slow" outputs every 250ms using this instant
                let mut tick = Instant::now();

                // EK1100 is first slave, EL2889 is second
                let mut el2889 = slow_outputs
                    .slave(&client_slow, 1)
                    .expect("EL2889 not present!");

                // Set initial output state
                el2889.io_raw_mut().1[0] = 0x01;
                el2889.io_raw_mut().1[1] = 0x80;

                loop {
                    slow_outputs.tx_rx(&client_slow).await.expect("TX/RX");

                    // Increment every output byte for every slave device by one
                    if tick.elapsed() > slow_duration {
                        tick = Instant::now();

                        let (_i, o) = el2889.io_raw_mut();

                        // Make a nice pattern on EL2889 LEDs
                        o[0] = o[0].rotate_left(1);
                        o[1] = o[1].rotate_right(1);
                    }

                    tfd.read();
                }
            })
            .unwrap();
        })
        .unwrap();

    let fast = thread_priority::ThreadBuilder::default()
        .name("fast-task")
        // Might need to set `<user> hard rtprio 99` and `<user> soft rtprio 99` in `/etc/security/limits.conf`
        // Check limits with `ulimit -Hr` or `ulimit -Sr`
        .priority(ThreadPriority::Crossplatform(
            ThreadPriorityValue::try_from(48u8).unwrap(),
        ))
        // NOTE: Requires a realtime kernel
        .policy(ThreadSchedulePolicy::Realtime(
            RealtimeThreadSchedulePolicy::Fifo,
        ))
        .spawn(move |_| {
            core_affinity::set_for_current(fast_core)
                .then_some(())
                .expect("Set fast thread core");

            futures_lite::future::block_on::<Result<(), Error>>(async {
                let mut fast_outputs = fast_outputs.into_op(&client).await.expect("PRE-OP -> OP");

                let fast_cycle_time = Duration::from_micros(INTERVAL);

                let mut tfd = TimerFd::new().unwrap();

                tfd.set_state(
                    TimerState::Periodic {
                        current: fast_cycle_time,
                        interval: fast_cycle_time,
                    },
                    SetTimeFlags::Default,
                );

                loop {
                    fast_outputs.tx_rx(&client).await.expect("TX/RX");

                    // Increment every output byte for every slave device by one
                    for mut slave in fast_outputs.iter(&client) {
                        let (_i, o) = slave.io_raw_mut();

                        for byte in o.iter_mut() {
                            *byte = byte.wrapping_add(1);
                        }
                    }

                    tfd.read();
                }
            })
            .unwrap();
        })
        .unwrap();

    slow.join().expect("slow task failed");
    fast.join().expect("fast task failed");

    Ok(())
}
