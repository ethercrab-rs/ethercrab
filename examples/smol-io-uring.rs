//! Use blocking io_uring-based TX/RX loop with async tasks executed with `smol`.
//!
//! This example pins the TX/RX loop to core 0, starts two other `smol` tasks on the main thread.
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
        MainDevice, MainDeviceConfig, PduStorage, SubDeviceGroup, Timeouts,
    };
    use futures_lite::StreamExt;
    use std::{
        sync::Arc,
        time::{Duration, Instant},
    };
    use thread_priority::{
        RealtimeThreadSchedulePolicy, ThreadPriority, ThreadPriorityValue, ThreadSchedulePolicy,
    };

    /// Maximum number of SubDevices that can be stored. This must be a power of 2 greater than 1.
    const MAX_SUBDEVICES: usize = 16;
    /// Maximum PDU data payload size - set this to the max PDI size or higher.
    const MAX_PDU_DATA: usize = PduStorage::element_size(1100);
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
        slow_outputs: SubDeviceGroup<2, 4>,
        /// EL2828. 1 item, 1 byte of PDI for 8 output bits.
        fast_outputs: SubDeviceGroup<1, 1>,
    }

    env_logger::Builder::from_env(Env::default().default_filter_or("info"))
        .format_timestamp(Some(TimestampPrecision::Nanos))
        .init();

    let interface = std::env::args()
        .nth(1)
        .expect("Provide network interface as first argument.");

    log::info!("Starting multiple groups demo...");
    log::info!(
        "Ensure an EK1100 or EK1501 is the first SubDevice, with an EL2828 and EL2889 following it"
    );
    log::info!("Run with RUST_LOG=ethercrab=debug or =trace for debug information");

    let (tx, rx, pdu_loop) = PDU_STORAGE.try_split().expect("can only split once");

    let core_ids = core_affinity::get_core_ids().expect("Couldn't get core IDs");

    let tx_rx_core = core_ids
        .first()
        .copied()
        .expect("At least one core is required. Are you running on a potato?");

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

    let maindevice = MainDevice::new(
        pdu_loop,
        Timeouts {
            // Enormous timeout so we can still keep going even with very high system load
            // preventing processing from happening.
            pdu: Duration::from_millis(1000),
            ..Timeouts::default()
        },
        MainDeviceConfig::default(),
    );

    let maindevice = Arc::new(maindevice);

    // Read configurations from SubDevice EEPROMs and configure devices.
    let groups = smol::block_on(maindevice.init::<MAX_SUBDEVICES, _>(
        ethercat_now,
        |groups: &Groups, subdevice| match subdevice.name() {
            "EL2889" | "EK1100" | "EK1501" => Ok(&groups.slow_outputs),
            "EL2828" => Ok(&groups.fast_outputs),
            _ => Err(Error::UnknownSubDevice),
        },
    ))
    .expect("Init");

    let Groups {
        slow_outputs,
        fast_outputs,
    } = groups;

    let maindevice_slow = maindevice.clone();

    let slow_task = smol::spawn(async move {
        let slow_outputs = slow_outputs
            .into_op(&maindevice_slow)
            .await
            .expect("PRE-OP -> OP");

        let mut slow_cycle_time = smol::Timer::interval(Duration::from_micros(INTERVAL));

        let slow_duration = Duration::from_millis(250);

        // Only update "slow" outputs every 250ms using this instant
        let mut tick = Instant::now();

        // EK1100 is first SubDevice, EL2889 is second
        let mut el2889 = slow_outputs
            .subdevice(&maindevice_slow, 1)
            .expect("EL2889 not present!");

        // Set initial output state
        el2889.io_raw_mut().1[0] = 0x01;
        el2889.io_raw_mut().1[1] = 0x80;

        loop {
            slow_outputs.tx_rx(&maindevice_slow).await.expect("TX/RX");

            // Increment every output byte for every SubDevice by one
            if tick.elapsed() > slow_duration {
                tick = Instant::now();

                let (_i, o) = el2889.io_raw_mut();

                // Make a nice pattern on EL2889 LEDs
                o[0] = o[0].rotate_left(1);
                o[1] = o[1].rotate_right(1);
            }

            slow_cycle_time.next().await;
        }
    });

    let fast_task = smol::spawn(async move {
        let mut fast_outputs = fast_outputs
            .into_op(&maindevice)
            .await
            .expect("PRE-OP -> OP");

        let mut fast_cycle_time = smol::Timer::interval(Duration::from_micros(INTERVAL));

        loop {
            fast_outputs.tx_rx(&maindevice).await.expect("TX/RX");

            // Increment every output byte for every SubDevice by one
            for mut subdevice in fast_outputs.iter(&maindevice) {
                let (_i, o) = subdevice.io_raw_mut();

                for byte in o.iter_mut() {
                    *byte = byte.wrapping_add(1);
                }
            }

            fast_cycle_time.next().await;
        }
    });

    smol::block_on(smol::future::race(slow_task, fast_task));

    Ok(())
}
