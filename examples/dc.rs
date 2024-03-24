//! Configure Distributed Clocks (DC) for EK1100 and a couple of other modules.
//!
//! Please note this example uses experimental features and should not be used as a reference for
//! other code. It is here (currently) primarily to help develop EtherCrab.

use env_logger::Env;
use ethercrab::{
    error::Error,
    std::{ethercat_now, tx_rx_task, tx_rx_task_io_uring},
    Client, ClientConfig, PduStorage, RegisterAddress, Timeouts,
};
use futures_lite::StreamExt;
use rustix::{fs::Timespec, thread::clock_nanosleep_absolute, time::ClockId};
use std::{
    fs::File,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};
use ta::indicators::ExponentialMovingAverage;
use ta::Next;
use thread_priority::{
    RealtimeThreadSchedulePolicy, ThreadPriority, ThreadPriorityValue, ThreadSchedulePolicy,
};

/// Maximum number of slaves that can be stored. This must be a power of 2 greater than 1.
const MAX_SLAVES: usize = 16;
const MAX_PDU_DATA: usize = PduStorage::element_size(1100);
const MAX_FRAMES: usize = 32;
const PDI_LEN: usize = 64;

static PDU_STORAGE: PduStorage<MAX_FRAMES, MAX_PDU_DATA> = PduStorage::new();

const CYCLIC_OP_ENABLE: u8 = 0b0000_0001;
const SYNC0_ACTIVATE: u8 = 0b0000_0010;
const SYNC1_ACTIVATE: u8 = 0b0000_0100;

#[allow(unused)]
mod lan9252 {
    #[repr(u16)]
    pub enum Lan9252Register {
        /// 12.14.24 PDI CONTROL REGISTER.
        ProcessDataInterface = 0x0140,
        /// 12.14.29 SYNC/LATCH PDI CONFIGURATION REGISTER.
        SyncLatchConfig = 0x0151,
    }

    #[derive(Debug, ethercrab_wire::EtherCrabWireRead)]
    #[repr(u8)]
    pub enum SyncLatchDrivePolarity {
        /// 00: Push-Pull Active Low.
        PushPullActiveLow = 0b00,
        /// 01: Open Drain (Active Low).
        OpenDrainActiveLow = 0b01,
        /// 10: Push-Pull Active High.
        PushPullActiveHigh = 0b10,
        /// 11: Open Source (Active High).
        OpenSourceActiveHigh = 0b11,
    }

    /// LAN9252 12.14.29 SYNC/LATCH PDI CONFIGURATION REGISTER
    #[derive(Debug, ethercrab_wire::EtherCrabWireRead)]
    #[wire(bytes = 1)]
    pub struct Lan9252Conf {
        #[wire(bits = 2)]
        sync0_drive_polarity: SyncLatchDrivePolarity,

        /// `true` = SYNC0 (output), `false` = `LATCH0` (input).
        #[wire(bits = 1)]
        sync0_latch0: bool,

        #[wire(bits = 1)]
        sync0_map: bool,

        #[wire(bits = 2)]
        sync1_drive_polarity: SyncLatchDrivePolarity,

        /// `true` = SYNC1 (output), `false` = `LATCH1` (input).
        #[wire(bits = 1)]
        sync1_latch1: bool,

        #[wire(bits = 1)]
        sync1_map: bool,
    }
}

use lan9252::*;

#[allow(unused)]
#[derive(Debug, ethercrab_wire::EtherCrabWireRead)]
#[wire(bytes = 2)]
pub struct SupportedModes {
    /// Bit 0 = 1: free run is supported.
    #[wire(bits = 1)]
    free_run: bool,
    /// Bit 1 = 1: Synchronous with SM 2 event is supported.
    #[wire(bits = 1)]
    sm2: bool,
    /// Bit 2-3 = 01: DC mode is supported.
    #[wire(bits = 2)]
    dc_supported: bool,
    /// Bit 4-5 = 10: Output shift with SYNC1 event (only DC mode).
    #[wire(bits = 2)]
    sync1: bool,
    /// Bit 14 = 1: dynamic times (measurement through writing of 0x1C32:08).
    #[wire(pre_skip = 8, bits = 1, post_skip = 1)]
    dynamic: bool,
}

const TICK_INTERVAL: Duration = Duration::from_millis(5);

fn main() -> Result<(), Error> {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    let interface = std::env::args()
        .nth(1)
        .expect("Provide network interface as first argument.");

    log::info!("Starting Distributed Clocks demo...");
    log::info!("Run with RUST_LOG=ethercrab=debug or =trace for debug information");

    let (tx, rx, pdu_loop) = PDU_STORAGE.try_split().expect("can only split once");

    let client = Arc::new(Client::new(
        pdu_loop,
        Timeouts {
            wait_loop_delay: Duration::from_millis(5),
            state_transition: Duration::from_secs(10),
            pdu: Duration::from_millis(2000),
            ..Timeouts::default()
        },
        ClientConfig {
            dc_static_sync_iterations: 10_000,
            ..ClientConfig::default()
        },
    ));

    let mut tick_interval = smol::Timer::interval(TICK_INTERVAL);

    let sync0_cycle_time = TICK_INTERVAL.as_nanos() as u64;
    // SYNC1 is not currently supported. Leave this set to zero.
    let sync1_cycle_time = 0;
    // Example shift: data will be ready half way through cycle
    let cycle_shift = (TICK_INTERVAL / 2).as_nanos() as u64;
    // let cycle_shift = 0;

    // smol::spawn(tx_rx_task(&interface, tx, rx).expect("spawn TX/RX task")).detach();
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
            // core_affinity::set_for_current(tx_rx_core)
            //     .then_some(())
            //     .expect("Set TX/RX thread core");

            // Blocking io_uring
            tx_rx_task_io_uring(&interface, tx, rx).expect("TX/RX task");
        })
        .unwrap();

    // Wait for TX/RX loop to start
    thread::sleep(Duration::from_millis(200));

    smol::block_on(async {
        let mut group = client
            .init_single_group::<MAX_SLAVES, PDI_LEN>(ethercat_now)
            .await
            .expect("Init");

        // The group will be in PRE-OP at this point

        for slave in group.iter(&client) {
            if slave.name() == "LAN9252-EVB-HBI" {
                // log::info!("Found LAN9252 in {:?} state", slave.status().await.ok());

                // Sync mode 02 = SYNC0
                slave
                    .sdo_write(0x1c32, 1, 2u16)
                    .await
                    .expect("Set sync mode");

                // ETG1020 calc and copy time
                let cal_and_copy_time = slave
                    .sdo_read::<u16>(0x1c32, 6)
                    .await
                    .expect("Calc and copy time");

                // Delay time
                let delay_time = slave.sdo_read::<u16>(0x1c32, 9).await.expect("Delay time");

                log::info!(
                    "LAN9252 calc time {} ns, delay time {} ns",
                    cal_and_copy_time,
                    delay_time,
                );

                // Adding this seems to make the second LAN9252 converge much more quickly
                slave
                    .sdo_write(0x1c32, 0x0a, TICK_INTERVAL.as_nanos() as u32)
                    .await
                    .expect("Set cycle time");

                let sync_type = slave.sdo_read::<u16>(0x1c32, 1).await?;
                let cycle_time = slave.sdo_read::<u32>(0x1c32, 2).await?;
                let min_cycle_time = slave.sdo_read::<u32>(0x1c32, 5).await?;
                let supported_sync_modes = slave.sdo_read::<SupportedModes>(0x1c32, 4).await?;
                log::info!("--> Outputs sync mode {sync_type}, cycle time {cycle_time} ns (min {min_cycle_time} ns), supported modes {supported_sync_modes:?}");

                let sync_type = slave.sdo_read::<u16>(0x1c33, 1).await?;
                let cycle_time = slave.sdo_read::<u32>(0x1c33, 2).await?;
                let min_cycle_time = slave.sdo_read::<u32>(0x1c33, 5).await?;
                let supported_sync_modes = slave.sdo_read::<SupportedModes>(0x1c33, 4).await?;
                log::info!("--> Inputs sync mode {sync_type}, cycle time {cycle_time} ns (min {min_cycle_time} ns), supported modes {supported_sync_modes:?}");

                let v = slave
                    .register_read::<Lan9252Conf>(Lan9252Register::SyncLatchConfig as u16)
                    .await
                    .expect("LAN9252 SyncLatchConfig");

                log::info!("--> LAN9252 config reg 0x0151:");
                log::info!("----> {:?}", v);
            }
        }

        log::info!("Group has {} slaves", group.len());

        let mut now = Instant::now();
        let start = Instant::now();
        let mut headers = false;
        // Compile time switch
        let debug_csv = option_env!("ETHERCRAB_CSV");

        let mut averages = Vec::new();

        for _ in 0..group.len() {
            averages.push(ExponentialMovingAverage::new(64).unwrap());
        }

        let mut group = group.into_pre_op_pdi(&client).await?;

        log::info!("Waiting for SubDevices to align");

        // Repeatedly send group PDI and sync frame to align all SubDevice clocks. We use an
        // exponential moving average of each SubDevice's deviation from the EtherCAT System Time
        // (the time in the DC reference SubDevice) and take the maximum deviation. When that is
        // below 100ns (arbitraily chosen value for this demo), we call the sync good enough and
        // exit the loop.
        loop {
            // Note this method is experimental and currently hidden from the crate docs.
            group.tx_rx_sync_dc(&client).await.expect("TX/RX");

            if now.elapsed() >= Duration::from_millis(25) {
                now = Instant::now();

                let mut row = Vec::with_capacity(group.len());

                if debug_csv.is_some() && !headers {
                    print!("t_ms");
                }

                for (s1, ema) in group.iter(&client).zip(averages.iter_mut()) {
                    let diff = match s1
                        .register_read::<u32>(RegisterAddress::DcSystemTimeDifference)
                        .await
                        .map(|value| {
                            let flag = 0b1u32 << 31;

                            let less_than = value & flag > 0;

                            let value = value & !flag;

                            if less_than {
                                -(value as i32)
                            } else {
                                value as i32
                            }
                        }) {
                        Ok(diff) => diff,
                        Err(Error::WorkingCounter { .. }) => 0,
                        Err(e) => return Err(e),
                    };

                    let ema_next = ema.next(diff as f64);

                    row.push([diff as f64, ema_next]);

                    log::debug!(
                        "--> Sys time {} offs {}, diff {} (EMA {:0.3})",
                        match s1.register_read::<u64>(RegisterAddress::DcSystemTime).await {
                            Ok(diff) => diff,
                            Err(Error::WorkingCounter { .. }) => 0,
                            Err(e) => return Err(e),
                        },
                        match s1
                            .register_read::<u64>(RegisterAddress::DcSystemTimeOffset)
                            .await
                        {
                            Ok(diff) => diff,
                            Err(Error::WorkingCounter { .. }) => 0,
                            Err(e) => return Err(e),
                        },
                        diff,
                        ema_next,
                    );

                    if debug_csv.is_some() && !headers {
                        print!(
                            ",{:#06x},{:#06x} EMA",
                            s1.configured_address(),
                            s1.configured_address()
                        );
                    }
                }

                if debug_csv.is_some() && !headers {
                    println!();
                }

                if debug_csv.is_some() {
                    println!(
                        "{},{}",
                        start.elapsed().as_millis(),
                        row.iter()
                            .flatten()
                            .map(|v| v.to_string())
                            .collect::<Vec<_>>()
                            .as_slice()
                            .join(","),
                    );
                }

                let max_deviation = row
                    .iter()
                    .map(|[_diff, diff_ema]| diff_ema.abs() as u32)
                    .max()
                    .unwrap_or(u32::MAX);

                // Less than 100ns max deviation.
                // <https://github.com/OpenEtherCATsociety/SOEM/issues/487#issuecomment-786245585>
                // mentions less than 100us as a good enough value as well.
                if max_deviation < 100 {
                    log::info!("Clocks settled after {} ms", start.elapsed().as_millis());

                    break;
                }

                headers = true;
            }

            tick_interval.next().await;
        }

        log::info!("Alignment done");

        // Now that clocks have synchronised, start cyclic operation by configuring SYNC0 start,
        // cycle time, etc. This should be quick enough that not much drift is observed during
        // config, and when `tx_rx_sync_dc` is called next (in a loop) it will resync the clocks.
        // This code must also be fast enough that all SubDevices are configured within the 100ms
        // first start offset set below.
        for slave in group.iter(&client) {
            if slave.dc_support().enhanced() {
                // Disable cyclic op, ignore WKC
                match slave
                    .register_write(RegisterAddress::DcSyncActive, 0u8)
                    .await
                {
                    Ok(_) | Err(Error::WorkingCounter { .. }) => (),
                    Err(e) => return Err(e),
                };

                let device_time = match slave
                    .register_read::<u64>(RegisterAddress::DcSystemTime)
                    .await
                {
                    Ok(t) => t,
                    // Ignore WKC, default to 0.
                    Err(Error::WorkingCounter { .. }) => 0,
                    Err(e) => return Err(e),
                };

                log::info!("Device time {}", device_time);

                // TODO: Support SYNC1. Only SYNC0 has been tested at time of writing.
                let true_cycle_time =
                    ((sync1_cycle_time / sync0_cycle_time) + 1) * sync0_cycle_time;

                let first_pulse_delay = Duration::from_millis(100).as_nanos() as u64;

                // Round first pulse time to a whole number of cycles
                let t = (device_time + first_pulse_delay) / true_cycle_time * true_cycle_time;

                // Add one more cycle plus user-configured cycle shift
                let t = t + true_cycle_time + cycle_shift;

                log::info!("Computed DC sync start time: {}", t);

                match slave
                    .register_write(RegisterAddress::DcSyncStartTime, t)
                    .await
                {
                    Ok(_) | Err(Error::WorkingCounter { .. }) => (),
                    Err(e) => return Err(e),
                };

                // Cycle time in nanoseconds
                match slave
                    .register_write(RegisterAddress::DcSync0CycleTime, sync0_cycle_time)
                    .await
                {
                    Ok(_) | Err(Error::WorkingCounter { .. }) => (),
                    Err(e) => return Err(e),
                };

                // slave
                //     .register_write(RegisterAddress::DcSync1CycleTime, sync1_cycle_time)
                //     .await
                //     .expect("DcSync1CycleTime");

                match slave
                    .register_write(
                        RegisterAddress::DcSyncActive,
                        // CYCLIC_OP_ENABLE causes SM watchdog timeouts when going into OP
                        SYNC0_ACTIVATE | CYCLIC_OP_ENABLE,
                    )
                    .await
                {
                    Ok(_) | Err(Error::WorkingCounter { .. }) => (),
                    Err(e) => return Err(e),
                };
            }
        }

        let mut group = group
            .into_safe_op(&client)
            .await
            .expect("PRE-OP -> SAFE-OP");

        log::info!("SAFE-OP");

        // Provide valid outputs before transition. LAN9252 will timeout going into OP if outputs
        // are not present.
        // Note this method is experimental and currently hidden from the crate docs.
        group.tx_rx_sync_dc(&client).await.expect("TX/RX");

        // FIXME: Make going into OP not error with a watchdog timeout
        // let mut group = group.into_op(&client).await.expect("SAFE-OP -> OP");

        // log::info!("OP");

        #[derive(serde::Serialize)]
        struct PiStat {
            ecat_time: u64,
            cycle_start_offset: u64,
            next_iter_wait: i64,
            shift_time: f32,
            adjusted_wait: i64,
            pi_out: i64,
            pi_offset: i64,
        }

        let mut pi_stats = csv::Writer::from_writer(File::create("dc-pi.csv").expect("Open CSV"));

        let term = Arc::new(AtomicBool::new(false));
        signal_hook::flag::register(signal_hook::consts::SIGINT, Arc::clone(&term))
            .expect("Register hook");

        let mut print_tick = Instant::now();

        // PI controller (no D term)
        let mut dc_pi = {
            let max_value = 50_000 as f32;

            let mut pi = pid::Pid::<f32>::new(cycle_shift as f32, max_value);

            pi.i(1000.0, max_value);

            pi
        };

        let mut offset = 0.0 as f32;

        loop {
            let now = current_time();

            // Note this method is experimental and currently hidden from the crate docs.
            let (_wkc, dc_time) = group.tx_rx_sync_dc(&client).await.expect("TX/RX");

            let this_cycle_delay = if let Some(dc_time) = dc_time {
                // Nanoseconds from the start of the cycle. This works because the first SYNC0 pulse
                // time is rounded to a whole number of `sync0_cycle_time`-length cycles.
                let cycle_start_offset = dc_time % sync0_cycle_time;

                // Use the PI loop to try and converge the difference between the measured offset
                // from the SYNC0 pulse and the `offset` value. This should result in a value
                // near/around `cycle_shift`, which we can then add `sync0_cycle_time` to to
                // calculate the next time we should send process data.
                let change = dc_pi
                    .next_control_output(cycle_start_offset as f32 + offset)
                    .output;

                offset += change;

                // let remaining_time = sync0_cycle_time as i64;

                // assert!(remaining_time >= 0, "Not enough cycle left");

                // // tick_interval.set_interval(Duration::from_nanos(remaining_time as u64));

                let time_to_next_iter = sync0_cycle_time as i64 + offset as i64;

                let stat = PiStat {
                    ecat_time: dc_time,
                    cycle_start_offset,
                    pi_out: change as i64,
                    pi_offset: offset as i64,
                    // This value should hover around the cycle offset, at time of writing 2500us,
                    // or half the 5ms cycle time.
                    shift_time: (cycle_start_offset as f32 + offset) / 1000.0,
                    adjusted_wait: sync0_cycle_time as i64 + offset as i64,
                    next_iter_wait: time_to_next_iter,
                };

                // // Sleep for an extra bit of time to align us with the desired cycle shift. E.g. if
                // // we're sending the frame at 200us offset from SYNC0 and we have a 500us offset,
                // // sleep for 300us to put is in the right spot. The loop delay below takes care of
                // // the cycle-cycle delay.
                // smol::Timer::after(Duration::from_nanos(
                //     cycle_start_offset.saturating_sub(offset as u64),
                // ))
                // .await;

                if print_tick.elapsed() > Duration::from_secs(1) {
                    print_tick = Instant::now();

                    log::info!(
                        "Offset from start of cycle {}, PI output {}, offset {}, {} ms from cycle start, next tick in {:0.3} ms",
                        cycle_start_offset,
                        change,
                        offset,
                        (cycle_start_offset as f32 + offset) / 1000.0 / 1000.0,
                        (time_to_next_iter as f32) / 1000.0/1000.0
                    );
                }

                pi_stats.serialize(stat).ok();

                // TODO
                // add_nanos(now, sync0_cycle_time)

                // Duration::from_nanos(dbg!(sync0_cycle_time as i64 + offset as i64) as u64)
                // (sync0_cycle_time as i64 + offset as i64) as u64
                time_to_next_iter as u64
            } else {
                sync0_cycle_time
            };

            // for mut slave in group.iter(&client) {
            //     let (_i, o) = slave.io_raw_mut();

            //     for byte in o.iter_mut() {
            //         *byte = byte.wrapping_add(1);
            //     }
            // }

            // tick_interval.next().await;
            // smol::Timer::at(this_cycle_delay).await;

            let sleep_until = add_nanos(now, this_cycle_delay);

            clock_nanosleep_absolute(ClockId::Monotonic, &sleep_until).ok();

            // Hook signal so we can write CSV data before exiting
            if term.load(Ordering::Relaxed) {
                log::info!("Exiting...");

                pi_stats.flush().ok();

                break Ok(());
            }
        }
    })
}

/// Get the current monotonic system time in nanoseconds.
pub fn current_time() -> Timespec {
    rustix::time::clock_gettime(ClockId::Monotonic)
}

const NSEC_PER_SEC: i64 = 1_000_000_000;

fn add_nanos(mut current: Timespec, add_nanos: u64) -> Timespec {
    let add_nanos = add_nanos as i64;

    let mut nsec = add_nanos % NSEC_PER_SEC;
    let sec = (add_nanos - nsec) / NSEC_PER_SEC;

    current.tv_sec += sec;
    current.tv_nsec += nsec;

    if current.tv_nsec >= NSEC_PER_SEC {
        nsec = current.tv_nsec % NSEC_PER_SEC;
        current.tv_sec += (current.tv_nsec - nsec) / NSEC_PER_SEC;
        current.tv_nsec = nsec;
    }

    current
}
