//! Configure a Nanotec C5-E EtherCAT stepper drive.
//!
//! Motor used for testing is AS5918M2804-E with 500PPR encoder.

use crate::c5e::{C5e, Ds402State};
use env_logger::Env;
use ethercrab::{
    std::{ethercat_now, tx_rx_task},
    Client, ClientConfig, PduStorage, Timeouts,
};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::time::MissedTickBehavior;

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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
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

    tokio::spawn(tx_rx_task(&interface, tx, rx).expect("spawn TX/RX task"));

    let mut group = client
        .init_single_group::<MAX_SLAVES, PDI_LEN>(ethercat_now)
        .await
        .expect("Init");

    for slave in group.iter(&client) {
        C5e::configure(&slave).await?;
    }

    let mut group = group.into_op(&client).await.expect("PRE-OP -> OP");

    log::info!("Slaves moved to OP state");

    log::info!("Discovered {} slaves", group.len());

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

    // Run twice to prime PDI
    group.tx_rx(&client).await.expect("TX/RX");

    // Read cycle time from servo drive
    let cycle_time = {
        let slave = group.slave(&client, 0).unwrap();

        let base = slave.sdo_read::<u8>(0x60c2, 1).await?;
        let x10 = slave.sdo_read::<i8>(0x60c2, 2).await?;

        let base = f32::from(base);
        let x10 = 10.0f32.powi(i32::from(x10));

        let cycle_time_ms = (base * x10) * 1000.0;

        Duration::from_millis(unsafe { cycle_time_ms.round().to_int_unchecked() })
    };

    log::info!("Cycle time: {} ms", cycle_time.as_millis());

    let mut cyclic_interval = tokio::time::interval(cycle_time);
    cyclic_interval.set_missed_tick_behavior(MissedTickBehavior::Skip);

    let slave = group.slave(&client, 0).expect("No servo!");
    let mut servo = C5e::new(slave);

    let mut velocity: i32 = 0;

    let accel = 1;
    let max_vel = 300;

    let term = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(signal_hook::consts::SIGINT, Arc::clone(&term))
        .expect("Register hook");

    // Update state from drive
    group.tx_rx(&client).await.expect("TX/RX");
    log::info!("Current drive state {:?}", servo.current_state());

    loop {
        group.tx_rx(&client).await.expect("TX/RX");

        if let Some((prev_state, new_state)) = servo.state_change() {
            log::info!("State change {:?} -> {:?}", prev_state, new_state);

            match new_state {
                Ds402State::Fault => {
                    log::error!("Drive fault!");

                    velocity = 0;

                    servo.clear_fault();
                }
                Ds402State::NotReadyToSwitchOn => {
                    servo.shutdown().expect("Shutdown");
                }
                Ds402State::SwitchOnDisabled => {
                    servo.shutdown().expect("Shutdown 2");
                }
                Ds402State::ReadyToSwitchOn => {
                    servo.switch_on().expect("Switch on");
                }
                Ds402State::SwitchedOn => {
                    servo.enable_op().expect("Enable op");
                }
                Ds402State::OpEnabled => {
                    log::info!("Op is enabled");
                }
                // Ds402State::QuickStop => todo!(),
                s => log::info!("Unhandled state {:?}", s),
            }
        }

        if servo.current_state()? == Ds402State::OpEnabled {
            servo.set_velocity(velocity);

            // Normal operation: accelerate up to max speed
            if !term.load(Ordering::Relaxed) {
                if velocity < max_vel {
                    velocity += accel;
                }
            }
            // Stopping: decelerate down to 0 velocity
            else if velocity > 0 {
                velocity -= accel;
            }
            // Stopped; time to exit
            else {
                log::info!("Motor stopped");

                break;
            }
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

        cyclic_interval.tick().await;
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

    //     cyclic_interval.tick().await;
    // }

    log::info!("Drive is shut down");

    Ok(())
}
