//! Configure a Leadshine EtherCat EL7 series drive and turn the motor.
//!
//! This demonstrates using the DS402 state machine to operate a DS402 compliant servo drive.
//!
//! # Experimental
//!
//! Please note the `ds402` module is experimental and may change drastically at any time. It is
//! also not well tested so use at your own risk.

use env_logger::Env;
use ethercrab::{
    ds402::{Ds402, Ds402Sm},
    error::Error,
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

/// Maximum number of SubDevices that can be stored.
const MAX_SUBDEVICES: usize = 16;
/// Maximum PDU data payload size - set this to the max PDI size or higher.
const MAX_PDU_DATA: usize = PduStorage::element_size(1100);
/// Maximum number of EtherCAT frames that can be in flight at any one time.
const MAX_FRAMES: usize = 16;
/// Maximum total PDI length.
const PDI_LEN: usize = 64;

static PDU_STORAGE: PduStorage<MAX_FRAMES, MAX_PDU_DATA> = PduStorage::new();

#[tokio::main]
async fn main() -> Result<(), Error> {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    let interface = std::env::args()
        .nth(1)
        .expect("Provide network interface as first argument.");

    log::info!("Starting EC400 demo...");
    log::info!("Ensure an EC400 servo drive is the first and only SubDevice");
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
        .init_single_group::<MAX_SUBDEVICES, PDI_LEN>(ethercat_now)
        .await
        .expect("Init");

    for subdevice in group.iter(&client) {
        if subdevice.name() == "ELP-EC400S" {
            // CSV described a bit better in section 7.6.2.2 Related Objects of the manual
            subdevice.sdo_write(0x1600, 0, 0u8).await?;
            // Control word, u16
            // NOTE: The lower word specifies the field length
            subdevice.sdo_write(0x1600, 1, 0x6040_0010u32).await?;
            // Target velocity, i32
            subdevice.sdo_write(0x1600, 2, 0x60ff_0020u32).await?;
            subdevice.sdo_write(0x1600, 0, 2u8).await?;

            subdevice.sdo_write(0x1a00, 0, 0u8).await?;
            // Status word, u16
            subdevice.sdo_write(0x1a00, 1, 0x6041_0010u32).await?;
            // Actual position, i32
            subdevice.sdo_write(0x1a00, 2, 0x6064_0020u32).await?;
            // Actual velocity, i32
            subdevice.sdo_write(0x1a00, 3, 0x606c_0020u32).await?;
            subdevice.sdo_write(0x1a00, 0, 0x03u8).await?;

            subdevice.sdo_write(0x1c12, 0, 0u8).await?;
            subdevice.sdo_write(0x1c12, 1, 0x1600).await?;
            subdevice.sdo_write(0x1c12, 0, 1u8).await?;

            subdevice.sdo_write(0x1c13, 0, 0u8).await?;
            subdevice.sdo_write(0x1c13, 1, 0x1a00).await?;
            subdevice.sdo_write(0x1c13, 0, 1u8).await?;

            // Opmode - Cyclic Synchronous Position
            // subdevice.write_sdo(0x6060, 0, 0x08).await?;
            // Opmode - Cyclic Synchronous Velocity
            subdevice.sdo_write(0x6060, 0, 0x09u8).await?;
        }
    }

    let mut group = group.into_op(&client).await.expect("PRE-OP -> OP");

    log::info!("SubDevices moved to OP state");

    log::info!("Discovered {} SubDevices", group.len());

    for subdevice in group.iter(&client) {
        let (i, o) = subdevice.io_raw();

        log::info!(
            "-> SubDevice {:#06x} {} inputs: {} bytes, outputs: {} bytes",
            subdevice.configured_address(),
            subdevice.name(),
            i.len(),
            o.len()
        );
    }

    // Run twice to prime PDI
    group.tx_rx(&client).await.expect("TX/RX");

    // Read cycle time from servo drive
    let cycle_time = {
        let subdevice = group.subdevice(&client, 0).unwrap();

        let base = subdevice.sdo_read::<u8>(0x60c2, 1).await?;
        let x10 = subdevice.sdo_read::<i8>(0x60c2, 2).await?;

        let base = f32::from(base);
        let x10 = 10.0f32.powi(i32::from(x10));

        let cycle_time_ms = (base * x10) * 1000.0;

        Duration::from_millis(unsafe { cycle_time_ms.round().to_int_unchecked() })
    };

    log::info!("Cycle time: {} ms", cycle_time.as_millis());

    let mut cyclic_interval = tokio::time::interval(cycle_time);
    cyclic_interval.set_missed_tick_behavior(MissedTickBehavior::Skip);

    let subdevice = group.subdevice(&client, 0).expect("No servo!");
    let mut servo = Ds402Sm::new(Ds402::new(subdevice).expect("Failed to gather DS402"));

    let mut velocity: i32 = 0;

    let accel = 300;

    let term = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(signal_hook::consts::SIGINT, Arc::clone(&term))
        .expect("Register hook");

    loop {
        group.tx_rx(&client).await.expect("TX/RX");

        if servo.tick() {
            // // Opmode - Cyclic Synchronous Position
            // servo
            //     .sm
            //     .context()
            //     .subdevice
            //     .write_sdo(&client, 0x6060, 0, 0x08u8)
            //     .await?;

            let status = servo.status_word();
            let (i, o) = servo.subdevice().io_raw_mut();

            let (pos, vel) = {
                let pos = i32::from_le_bytes(i[2..=5].try_into().unwrap());
                let vel = i32::from_le_bytes(i[6..=9].try_into().unwrap());

                (pos, vel)
            };

            println!(
                "Position: {pos}, velocity: {vel}, status: {status:?} | {:?}",
                o
            );

            let pos_cmd = &mut o[2..=5];

            pos_cmd.copy_from_slice(&velocity.to_le_bytes());

            if term.load(Ordering::Relaxed) {
                if vel < 200_000 {
                    velocity += accel;
                }
            } else if vel > 0 {
                velocity -= accel;
            } else {
                break;
            }
        }

        cyclic_interval.tick().await;
    }

    log::info!("Servo stopped, shutting drive down");

    loop {
        group.tx_rx(&client).await.expect("TX/RX");

        if servo.tick_shutdown() {
            break;
        }

        let status = servo.status_word();
        let (i, o) = servo.subdevice().io_raw_mut();

        let (pos, vel) = {
            let pos = i32::from_le_bytes(i[2..=5].try_into().unwrap());
            let vel = i32::from_le_bytes(i[6..=9].try_into().unwrap());

            (pos, vel)
        };

        println!(
            "Position: {pos}, velocity: {vel}, status: {status:?} | {:?}",
            o
        );

        cyclic_interval.tick().await;
    }

    log::info!("Drive is shut down");

    Ok(())
}
