//! Configure Distributed Clocks (DC) for EK1100 and a couple of other modules.

use env_logger::Env;
use ethercrab::{
    error::Error, std::tx_rx_task, Client, ClientConfig, Command, PduStorage, RegisterAddress,
    SlaveGroupState, Timeouts,
};
use rustix::{fs::Timespec, time::ClockId};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::time::MissedTickBehavior;

/// Maximum number of slaves that can be stored. This must be a power of 2 greater than 1.
const MAX_SLAVES: usize = 16;
const MAX_PDU_DATA: usize = 1100;
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

const TICK_INTERVAL: Duration = Duration::from_millis(2);

#[tokio::main]
async fn main() -> Result<(), Error> {
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
            ..Timeouts::default()
        },
        ClientConfig {
            dc_static_sync_iterations: 10_000,
            ..ClientConfig::default()
        },
    ));

    let mut tick_interval = tokio::time::interval(TICK_INTERVAL);
    tick_interval.set_missed_tick_behavior(MissedTickBehavior::Skip);

    tokio::spawn(tx_rx_task(&interface, tx, rx).expect("spawn TX/RX task"));

    let mut group = client
        .init_single_group::<MAX_SLAVES, PDI_LEN>(|| {
            let Timespec { tv_sec, tv_nsec } = rustix::time::clock_gettime(ClockId::Monotonic);

            let t = (tv_sec * 1000 * 1000 * 1000 + tv_nsec) as u64;

            t.saturating_sub(946684800)
        })
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
        }
    }

    let client2 = client.clone();
    let group_len = group.len() as u16;

    for slave in group.iter(&client) {
        if slave.name() == "LAN9252-EVB-HBI" {
            // log::info!("Found LAN9252 in {:?} state", slave.status().await.ok());

            let sync_type = slave.sdo_read::<u16>(0x1c32, 1).await?;
            let cycle_time = slave.sdo_read::<u32>(0x1c32, 2).await?;
            let min_cycle_time = slave.sdo_read::<u32>(0x1c32, 5).await?;
            let supported_sync_modes = slave.sdo_read::<SupportedModes>(0x1c32, 4).await?;
            log::info!("Outputs sync mode {sync_type}, cycle time {cycle_time} ns (min {min_cycle_time} ns), supported modes {supported_sync_modes:?}");

            let sync_type = slave.sdo_read::<u16>(0x1c33, 1).await?;
            let cycle_time = slave.sdo_read::<u32>(0x1c33, 2).await?;
            let min_cycle_time = slave.sdo_read::<u32>(0x1c33, 5).await?;
            let supported_sync_modes = slave.sdo_read::<SupportedModes>(0x1c33, 4).await?;
            log::info!("Inputs sync mode {sync_type}, cycle time {cycle_time} ns (min {min_cycle_time} ns), supported modes {supported_sync_modes:?}");

            let v = slave
                .register_read::<Lan9252Conf>(Lan9252Register::SyncLatchConfig as u16)
                .await
                .expect("LAN9252 SyncLatchConfig");

            log::info!("LAN9252 config reg 0x0151: {:?}", v);
        }

        if slave.dc_support().any() {
            // log::info!("{} supports DC", slave.name());

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

            let sync0_cycle_time = TICK_INTERVAL.as_nanos() as u64;
            let sync1_cycle_time = TICK_INTERVAL.as_nanos() as u64;
            let cycle_shift = (TICK_INTERVAL / 2).as_nanos() as u64;

            let true_cycle_time = ((sync1_cycle_time / sync0_cycle_time) + 1) * sync0_cycle_time;

            let first_pulse_delay = Duration::from_millis(100).as_nanos() as u64;

            let t = (device_time + first_pulse_delay) / true_cycle_time * true_cycle_time
                + true_cycle_time
                + cycle_shift;

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

    // Start continuous drift compensation in PRE-OP
    tokio::spawn(async move {
        let mut sync_tick = tokio::time::interval(TICK_INTERVAL);
        sync_tick.set_missed_tick_behavior(MissedTickBehavior::Skip);

        loop {
            let system_time = {
                let Timespec { tv_sec, tv_nsec } = rustix::time::clock_gettime(ClockId::Monotonic);

                tv_sec * 1000 * 1000 * 1000 + tv_nsec
            } as u64;

            // EtherCAT's epoch is 2000-01-01, not 1970-01-01
            let system_time = system_time.saturating_sub(946684800);

            // TODO: Find first DC slave instead of hardcoded address.
            // Dynamic drift compensation. Assumes first device supports DC
            Command::frmw(0x1000, RegisterAddress::DcSystemTime.into())
                .wrap(&client2)
                .with_wkc(group_len)
                .send(system_time)
                .await
                .expect("Sync tick");

            sync_tick.tick().await;
        }
    });

    log::info!("Group has {} slaves", group.len());

    let mut now = Instant::now();
    let start = Instant::now();
    let mut headers = false;
    // Compile time switch
    let debug_csv = option_env!("ETHERCRAB_CSV");

    loop {
        // group.tx_rx(&client).await.expect("TX/RX");

        if now.elapsed() >= Duration::from_millis(25) {
            now = Instant::now();

            log::debug!("Stat");

            let mut row = Vec::with_capacity(group.len());

            if debug_csv.is_some() && !headers {
                print!("t_ms");
            }

            for s1 in group.iter(&client) {
                // let s1 = group.slave(&client, 1).unwrap();

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

                row.push(diff);

                log::debug!(
                    "--> Sys time {} offs {}, diff {}",
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
                );

                if !headers {
                    print!(",{:#06x}", s1.configured_address());
                }
            }

            if debug_csv.is_some() && !headers {
                println!();
            }

            if debug_csv.is_some() {
                println!(
                    "{},{}",
                    start.elapsed().as_millis(),
                    row.into_iter()
                        .map(|v| v.to_string())
                        .collect::<Vec<_>>()
                        .as_slice()
                        .join(",")
                );
            }

            headers = true;
        }

        tick_interval.tick().await;
    }

    log::info!("Sync done");

    let mut group = group
        .into_safe_op(&client)
        .await
        .expect("PRE-OP -> SAFE-OP");

    // // Provide valid outputs before transition. LAN9252 will timeout going into OP if outputs are
    // // not present.
    // group.tx_rx(&client).await.expect("TX/RX");

    let mut group = group.into_op(&client).await.expect("SAFE-OP -> OP");

    loop {
        group.tx_rx(&client).await.expect("TX/RX");

        // for mut slave in group.iter(&client) {
        //     let (_i, o) = slave.io_raw_mut();

        //     for byte in o.iter_mut() {
        //         *byte = byte.wrapping_add(1);
        //     }
        // }

        tick_interval.tick().await;
    }
}
