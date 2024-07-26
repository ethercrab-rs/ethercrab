//! Configure a Nanotec C5-E EtherCAT stepper drive.
//!
//! Motor used for testing is AS5918M2804-E with 500PPR encoder.

use anyhow::Context;
use env_logger::Env;
use ethercrab::{
    ds402::{Ds402, Ds402Sm, StatusWord},
    std::{ethercat_now, tx_rx_task},
    MainDevice, MainDeviceConfig, PduStorage, Timeouts,
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

/// C5-E manual page 127 "Error number"
#[derive(ethercrab::EtherCrabWireRead, Debug)]
#[allow(unused)]
#[wire(bytes = 4)]
struct C5Error {
    #[wire(bytes = 2)]
    pub code: u16,
    #[wire(bytes = 1)]
    pub class: u8,
    #[wire(bytes = 1)]
    pub number: u8,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    let interface = std::env::args()
        .nth(1)
        .expect("Provide network interface as first argument.");

    log::info!("Starting C5-E demo...");
    log::info!("Run with RUST_LOG=ethercrab=debug or =trace for debug information");

    let (tx, rx, pdu_loop) = PDU_STORAGE.try_split().expect("can only split once");

    let maindevice = Arc::new(MainDevice::new(
        pdu_loop,
        Timeouts {
            wait_loop_delay: Duration::from_millis(2),
            mailbox_response: Duration::from_millis(1000),
            ..Default::default()
        },
        MainDeviceConfig::default(),
    ));

    tokio::spawn(tx_rx_task(&interface, tx, rx).expect("spawn TX/RX task"));

    let mut group = maindevice
        .init_single_group::<MAX_SUBDEVICES, PDI_LEN>(ethercat_now)
        .await
        .expect("Init");

    for subdevice in group.iter(&maindevice) {
        // Assuming all connected SubDevices are C5-Es here

        // Manual section 4.8 Setting the motor data
        // 1.8deg step, so 50 pole pairs
        subdevice
            .sdo_write(0x2030, 0, 50u32)
            .await
            .context("pole pairs")?;
        // Max motor current in mA.
        subdevice
            .sdo_write(0x2031, 0, 1000u32)
            .await
            .context("max current")?;
        // Rated motor current in mA
        subdevice
            .sdo_write(0x6075, 0, 2820u32)
            .await
            .context("rated currnet")?;
        // Max motor current, % of rated current in milli-percent, i.e. 1000 is 100%
        subdevice
            .sdo_write(0x6073, 0, 1000u16)
            .await
            .context("current %")?;
        // Max motor current max duration in ms
        subdevice
            .sdo_write(0x203b, 02, 100u32)
            .await
            .context("max current duration")?;
        // Motor type: stepper
        subdevice
            .sdo_write(0x3202, 00, 0x08u32)
            .await
            .context("set motor type")?;
        // Test motor has 500ppr incremental encoder, differential
        subdevice
            .sdo_write(0x2059, 00, 0x0u32)
            .await
            .context("encoder kind")?;
        // Set velocity unit to RPM (factory default)
        subdevice
            .sdo_write(0x60a9, 00, 0x00B44700u32)
            .await
            .context("velocity unit RPM")?;

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
        subdevice.sdo_write(0x1c12, 1, 0x1600u16).await?;
        subdevice.sdo_write(0x1c12, 0, 1u8).await?;

        subdevice.sdo_write(0x1c13, 0, 0u8).await?;
        subdevice.sdo_write(0x1c13, 1, 0x1a00u16).await?;
        subdevice.sdo_write(0x1c13, 0, 1u8).await?;

        // Opmode - Cyclic Synchronous Position
        // subdevice.write_sdo(0x6060, 0, 0x08).await?;
        // Opmode - Cyclic Synchronous Velocity
        subdevice.sdo_write(0x6060, 0, 0x09u8).await?;
    }

    let mut group = group.into_op(&maindevice).await.expect("PRE-OP -> OP");

    log::info!("SubDevices moved to OP state");

    log::info!("Discovered {} SubDevices", group.len());

    for subdevice in group.iter(&maindevice) {
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
    group.tx_rx(&maindevice).await.expect("TX/RX");

    // Read cycle time from servo drive
    let cycle_time = {
        let subdevice = group.subdevice(&maindevice, 0).unwrap();

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

    let subdevice = group.subdevice(&maindevice, 0).expect("No servo!");
    let mut servo = Ds402Sm::new(Ds402::new(subdevice).expect("Failed to gather DS402"));

    let mut velocity: i32 = 0;

    let accel = 1;
    let max_vel = 100;

    let term = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(signal_hook::consts::SIGINT, Arc::clone(&term))
        .expect("Register hook");

    loop {
        group.tx_rx(&maindevice).await.expect("TX/RX");

        if servo.tick() {
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

            let vel_cmd = &mut o[2..=5];

            vel_cmd.copy_from_slice(&velocity.to_le_bytes());

            if status.contains(StatusWord::FAULT) {
                let sl = servo.subdevice();

                let num_errors = sl
                    .sdo_read::<u8>(0x1003, 0)
                    .await
                    .context("Read error count")?;

                log::error!("Fault! ({})", num_errors);

                for idx in 1..=num_errors {
                    let code = sl
                        .sdo_read::<C5Error>(0x1003, idx)
                        .await
                        .context("Read error code")?;

                    log::error!("--> {:?}", code);
                }

                break;
            }

            // Normal operation: accelerate up to max speed
            if !term.load(Ordering::Relaxed) {
                if vel < max_vel {
                    velocity += accel;
                }
            }
            // Slow down to a stop when Ctrl + C is pressed
            else if vel > 0 {
                velocity -= accel;
            }
            // Deceleration is done, we can now exit this loop
            else {
                log::info!("Stopping...");

                break;
            }
        }

        cyclic_interval.tick().await;
    }

    log::info!("Servo stopped, shutting drive down");

    loop {
        group.tx_rx(&maindevice).await.expect("TX/RX");

        if servo.tick_shutdown() {
            break;
        }

        let status = servo.status_word();
        let (i, o) = servo.subdevice().io_raw_mut();

        // In fault state, so don't bother trying to shut down gracefully.
        if status.contains(StatusWord::FAULT) {
            break;
        }

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
