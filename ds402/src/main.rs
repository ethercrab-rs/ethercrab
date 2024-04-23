//! Configure a Nanotec C5-E EtherCAT stepper drive.
//!
//! Motor used for testing is AS5918M2804-E with 500PPR encoder.

use crate::c5e::{C5e, DesiredState, Ds402State};
use env_logger::Env;
use ethercrab::{
    error::Error,
    slave_group::{CycleInfo, DcConfiguration},
    std::{ethercat_now, tx_rx_task},
    Client, ClientConfig, DcSync, PduStorage, RegisterAddress, Timeouts,
};
use futures_lite::StreamExt;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use ta::{indicators::ExponentialMovingAverage, Next};

mod c5e;

/// Maximum number of slaves that can be stored.
const MAX_SLAVES: usize = 16;
/// Maximum PDU data payload size - set this to the max PDI size or higher.
const MAX_PDU_DATA: usize = PduStorage::element_size(1100);
/// Maximum number of EtherCAT frames that can be in flight at any one time.
const MAX_FRAMES: usize = 16;
/// Maximum total PDI length.
const PDI_LEN: usize = 64;

static PDU_STORAGE: PduStorage<MAX_FRAMES, MAX_PDU_DATA> = PduStorage::new();

const TICK_INTERVAL: Duration = Duration::from_millis(1);

/// Convert a Px.xxx parameter number into an EtherCAT SDO address according to 12.4.3.2 in the
/// manual.
fn param_to_object(group: u8, param: u8) -> u16 {
    // Mapping between Px.xxx and EtherCAT objects described in section 12.4.3.2.
    let base = 0x2000;
    // The Px part
    let group = u16::from(group & 0x0f) << 8;
    // The value after the dot
    let param = param;

    let ecat_object = base | group | u16::from(param);

    ecat_object
}

#[tokio::main]
async fn main() -> Result<(), ethercrab::error::Error> {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    let interface = std::env::args()
        .nth(1)
        .expect("Provide network interface as first argument.");

    log::info!("Starting C5-E demo...");
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

    let mut tick_interval = smol::Timer::interval(TICK_INTERVAL);

    tokio::spawn(tx_rx_task(&interface, tx, rx).expect("spawn TX/RX task"));

    let mut group = client
        .init_single_group::<MAX_SLAVES, PDI_LEN>(ethercat_now)
        .await
        .expect("Init");

    for mut slave in group.iter(&client) {
        // CSV described a bit better in section 7.6.2.2 Related Objects of the manual
        slave.sdo_write(0x1600, 0, 0u8).await?;
        // Control word, u16
        // NOTE: The lower word specifies the field length
        slave.sdo_write(0x1600, 1, 0x6040_0010u32).await?;
        // Target velocity, i32
        slave.sdo_write(0x1600, 2, 0x60ff_0020u32).await?;
        slave.sdo_write(0x1600, 0, 2u8).await?;

        slave.sdo_write(0x1a00, 0, 0u8).await?;
        // Status word, u16
        slave.sdo_write(0x1a00, 1, 0x6041_0010u32).await?;
        // Actual position, i32
        slave.sdo_write(0x1a00, 2, 0x6064_0020u32).await?;
        // Actual velocity, i32
        slave.sdo_write(0x1a00, 3, 0x606c_0020u32).await?;
        slave.sdo_write(0x1a00, 0, 0x03u8).await?;

        slave.sdo_write(0x1c12, 0, 0u8).await?;
        slave.sdo_write(0x1c12, 1, 0x1600u16).await?;
        slave.sdo_write(0x1c12, 0, 1u8).await?;

        slave.sdo_write(0x1c13, 0, 0u8).await?;
        slave.sdo_write(0x1c13, 1, 0x1a00u16).await?;
        slave.sdo_write(0x1c13, 0, 1u8).await?;

        // Opmode - Cyclic Synchronous Position
        // slave.sdo_write(0x6060, 0, 0x08u8).await?;
        // NOTE: ESI file only specifies CSP in `InitCmd`.
        // Opmode - Cyclic Synchronous Velocity
        slave.sdo_write(0x6060, 0, 0x09u8).await?;

        // <InitCmd>s from ESI
        slave.sdo_write(0x60C2, 1, 0x01u8).await?;
        slave.sdo_write(0x60C2, 2, 0xFDu8).await?;

        // Sync mode 02 = SYNC0
        slave
            .sdo_write(0x1c32, 1, 2u16)
            .await
            .expect("Set sync mode");

        // Adding this seems to make the second LAN9252 converge much more quickly
        slave
            .sdo_write(0x1c32, 0x0a, TICK_INTERVAL.as_nanos() as u32)
            .await
            .expect("Set cycle time");

        // 0x0300 in ESI file; enable only SYNC0 and DC
        slave.set_dc_sync(DcSync::Sync0);

        // ASDA-B3 DIx functional planning P2.010 - P2.017
        for i in 0..8u8 {
            let ecat_object = param_to_object(2, 10 + i);

            log::info!(
                "DI{}, conv to ECAT obj {:#06x} -> {:#06x}",
                i,
                ecat_object,
                // slave.register_read::<u16>(i).await.unwrap_or(u16::MAX)
                slave.sdo_read::<u16>(ecat_object, 0).await.expect("Read")
            );
        }

        // ---
        // DELETEME: DANGER: DISABLING EMERGENCY STOP DI3 INPUT FOR TESTING ONLY
        // ---
        log::warn!("DANGER: DISABLING EMERGENCY STOP INPUT. USE FOR TESTING ONLY.");
        slave
            .sdo_write(param_to_object(2, 10 + 3), 0, 0x0100)
            .await
            .expect("Disable EMGS input");
    }

    log::info!("Group has {} slaves", group.len());

    let mut averages = Vec::new();

    for _ in 0..group.len() {
        averages.push(ExponentialMovingAverage::new(64).unwrap());
    }

    log::info!("Moving into PRE-OP with PDI");

    let mut group = group.into_pre_op_pdi(&client).await?;

    log::info!("Done. PDI available. Waiting for SubDevices to align");

    let start = Instant::now();

    // Repeatedly send group PDI and sync frame to align all SubDevice clocks. We use an
    // exponential moving average of each SubDevice's deviation from the EtherCAT System Time
    // (the time in the DC reference SubDevice) and take the maximum deviation. When that is
    // below 100ns (arbitraily chosen value for this demo), we call the sync good enough and
    // exit the loop.
    loop {
        group.tx_rx_sync_system_time(&client).await.expect("TX/RX");

        let mut max_deviation = 0;

        for (s1, ema) in group.iter(&client).zip(averages.iter_mut()) {
            let diff = match s1
                .register_read::<u32>(RegisterAddress::DcSystemTimeDifference)
                .await
            {
                Ok(value) =>
                // The returned value is NOT in two's compliment, rather the upper bit specifies
                // whether the number in the remaining bits is odd or even, so we convert the
                // value to `i32` using that logic here.
                {
                    let flag = 0b1u32 << 31;

                    if value >= flag {
                        // Strip off negative flag bit and negate value as normal
                        -((value & !flag) as i32)
                    } else {
                        value as i32
                    }
                }
                Err(Error::WorkingCounter { .. }) => 0,
                Err(e) => return Err(e),
            };

            let ema_next = ema.next(diff as f64);

            max_deviation = max_deviation.max(ema_next.abs() as u32);
        }

        log::debug!("--> Max deviation {} ns", max_deviation);

        // Less than 100ns max deviation as an example threshold.
        // <https://github.com/OpenEtherCATsociety/SOEM/issues/487#issuecomment-786245585>
        // mentions less than 100us as a good enough value as well.
        if max_deviation < 100 {
            log::info!("Clocks settled after {} us", start.elapsed().as_micros());

            break;
        }

        tick_interval.next().await;
    }

    log::info!("Alignment done");

    // SubDevice clocks are aligned. We can turn DC on now.
    let group = group
        .configure_dc_sync(
            &client,
            DcConfiguration {
                // Start SYNC0 100ms in the future
                start_delay: Duration::from_millis(100),
                // SYNC0 period should be the same as the process data loop in most cases
                sync0_period: TICK_INTERVAL,
                sync0_shift: Duration::from_micros(400),
            },
        )
        .await?;

    group
        .tx_rx_sync_system_time(&client)
        .await
        .expect("Initial sync");

    let group = group
        .into_safe_op(&client)
        .await
        .expect("PRE-OP -> SAFE-OP");

    log::info!("SAFE-OP");

    let term = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(signal_hook::consts::SIGINT, Arc::clone(&term))
        .expect("Register hook");

    // Request OP state without waiting for all SubDevices to reach it. Allows the immediate
    // start of the process data cycle, which is required when DC sync is used, otherwise
    // SubDevices never reach OP, most often timing out with a SyncManagerWatchdog error.
    let group = group.request_into_op(&client).await.expect("SAFE-OP -> OP");

    log::info!("OP requested");

    let op_request = Instant::now();

    // Send PDI and check group state until all SubDevices enter OP state. At this point, we can
    // exit this loop and enter the main process data loop that does not have the state check
    // overhead present here.
    while !group.all_op(&client).await? {
        let now = Instant::now();

        let (
            _wkc,
            CycleInfo {
                next_cycle_wait, ..
            },
        ) = group.tx_rx_dc(&client).await.expect("TX/RX");

        smol::Timer::at(now + next_cycle_wait).await;
    }

    log::info!(
        "All SubDevices entered OP in {} us",
        op_request.elapsed().as_micros()
    );

    let mut servo = C5e::new(group.slave(&client, 0).expect("No subdevice!"));

    let accel = 1;
    let max_vel = 300;
    let mut velocity = 0;

    loop {
        let now = Instant::now();

        let (
            _wkc,
            CycleInfo {
                next_cycle_wait, ..
            },
        ) = group.tx_rx_dc(&client).await.expect("TX/RX");

        if let Some(Ds402State::Fault) = servo.power_state_machine() {
            let sl = servo.subdevice();

            log::error!(
                "General error code {:#010b}",
                sl.sdo_read::<u8>(0x1001, 0).await.unwrap_or(0xff)
            );

            // let num_errors = sl
            //     .sdo_read::<u8>(0x1003, 0)
            //     .await
            //     .expect("Read error count");

            // log::error!("Fault! ({})", num_errors);

            // for idx in 1..=num_errors {
            //     let code = sl
            //         .sdo_read::<u32>(0x1003, idx)
            //         .await
            //         .expect("Read error code");

            //     log::error!("--> {:#010x}", code);
            // }

            // log::info!("Clearing fault");

            // servo.clear_fault();
        }

        if servo.current_state()? == Ds402State::OpEnabled
            && servo.desired_state() == DesiredState::Op
        {
            servo.set_velocity(velocity);

            // Normal operation: accelerate up to max speed
            if !term.load(Ordering::Relaxed) {
                if velocity < max_vel {
                    // velocity += accel;
                }
            }
            // Stopping: decelerate down to 0 velocity
            else if velocity > 0 {
                velocity -= accel;
            }
            // Stopped; time to exit
            else {
                log::info!("Motor stopped");

                servo.set_desired_state(DesiredState::Shutdown);
            }
        } else if term.load(Ordering::Relaxed)
            && servo.current_state()? == Ds402State::NotReadyToSwitchOn
            && servo.desired_state() == DesiredState::Shutdown
        {
            log::info!("Drive is shut down");

            break;
        } else if term.load(Ordering::Relaxed) {
            log::info!("Exiting");

            break;
        }

        servo.update_outputs();

        //     if status.contains(StatusWord::FAULT) {
        //         let sl = servo.slave();

        //         let num_errors = sl
        //             .sdo_read::<u8>(0x1003, 0)
        //             .await
        //             .context("Read error count")?;

        //         log::error!("Fault! ({})", num_errors);

        //         for idx in 1..=num_errors {
        //             let code = sl
        //                 .sdo_read::<C5Error>(0x1003, idx)
        //                 .await
        //                 .context("Read error code")?;

        //             log::error!("--> {:?}", code);
        //         }

        //         break;
        //     }

        smol::Timer::at(now + next_cycle_wait).await;
    }

    log::info!("Servo stopped, shutting drive down");

    // loop {
    //     group.tx_rx(&client).await.expect("TX/RX");

    //     if servo.tick_shutdown() {
    //         break;
    //     }

    //     let status = servo.status_word();
    //     let (i, o) = servo.slave().io_raw_mut();

    //     // In fault state, so don't bother trying to shut down gracefully.
    //     if status.contains(StatusWord::FAULT) {
    //         break;
    //     }

    //     let (pos, vel) = {
    //         let pos = i32::from_le_bytes(i[2..=5].try_into().unwrap());
    //         let vel = i32::from_le_bytes(i[6..=9].try_into().unwrap());

    //         (pos, vel)
    //     };

    //     println!(
    //         "Position: {pos}, velocity: {vel}, status: {status:?} | {:?}",
    //         o
    //     );

    //     tick_interval.tick().await;
    // }

    log::info!("Drive is shut down");

    Ok(())
}
