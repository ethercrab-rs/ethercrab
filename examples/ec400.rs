//! Configure a Leadshine EtherCat EL7 series drive and turn the motor.

use ethercrab::{
    ds402::{Ds402, Ds402Sm},
    error::Error,
    std::tx_rx_task,
    Client, ClientConfig, PduStorage, SlaveGroup, SlaveState, SubIndex, Timeouts,
};
use futures_lite::StreamExt;
use smol::LocalExecutor;
use smol::Timer;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

#[cfg(target_os = "windows")]
// ASRock NIC
// const INTERFACE: &str = "TODO";
// // USB NIC
// const INTERFACE: &str = "\\Device\\NPF_{DCEDC919-0A20-47A2-9788-FC57D0169EDB}";
// Lenovo USB-C NIC
const INTERFACE: &str = "\\Device\\NPF_{CC0908D5-3CB8-46D6-B8A2-575D0578008D}";
// Silver USB NIC
// const INTERFACE: &str = "\\Device\\NPF_{CC0908D5-3CB8-46D6-B8A2-575D0578008D}";
#[cfg(not(target_os = "windows"))]
const INTERFACE: &str = "eth1";

const MAX_SLAVES: usize = 16;
const MAX_PDU_DATA: usize = 1100;
const MAX_FRAMES: usize = 16;
const PDI_LEN: usize = 64;

static PDU_STORAGE: PduStorage<MAX_FRAMES, MAX_PDU_DATA> = PduStorage::new();

async fn main_inner(ex: &LocalExecutor<'static>) -> Result<(), Error> {
    log::info!("Starting SDO demo...");

    let (tx, rx, pdu_loop) = PDU_STORAGE.try_split().expect("can only split once");

    let client = Arc::new(Client::new(
        pdu_loop,
        Timeouts::default(),
        ClientConfig::default(),
    ));

    ex.spawn(tx_rx_task(INTERFACE, tx, rx).expect("spawn TX/RX task"))
        .detach();

    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();

    ctrlc::set_handler(move || {
        r.store(false, Ordering::SeqCst);
    })
    .expect("Error setting Ctrl-C handler");

    // let num_slaves = client.num_slaves();

    let groups = SlaveGroup::<MAX_SLAVES, PDI_LEN>::new(|slave| {
        Box::pin(async {
            // --- Reads ---

            // // Name
            // dbg!(slave
            //     .read_sdo::<heapless::String<64>>(0x1008, SdoAccess::Index(0))
            //     .await
            //     .unwrap());

            // // Software version. For AKD, this should equal "M_01-20-00-003"
            // dbg!(slave
            //     .read_sdo::<heapless::String<64>>(0x100a, SdoAccess::Index(0))
            //     .await
            //     .unwrap());

            // --- Writes ---

            log::info!("Found {}", slave.name());

            if slave.name() == "ELP-EC400S" {
                // CSV described a bit better in section 7.6.2.2 Related Objects of the manual
                slave.write_sdo(0x1600, SubIndex::Index(0), 0u8).await?;
                // Control word, u16
                // NOTE: The lower word specifies the field length
                slave
                    .write_sdo(0x1600, SubIndex::Index(1), 0x6040_0010u32)
                    .await?;
                // Target velocity, i32
                slave
                    .write_sdo(0x1600, SubIndex::Index(2), 0x60ff_0020u32)
                    .await?;
                slave.write_sdo(0x1600, SubIndex::Index(0), 2u8).await?;

                slave.write_sdo(0x1a00, SubIndex::Index(0), 0u8).await?;
                // Status word, u16
                slave
                    .write_sdo(0x1a00, SubIndex::Index(1), 0x6041_0010u32)
                    .await?;
                // Actual position, i32
                slave
                    .write_sdo(0x1a00, SubIndex::Index(2), 0x6064_0020u32)
                    .await?;
                // Actual velocity, i32
                slave
                    .write_sdo(0x1a00, SubIndex::Index(3), 0x606c_0020u32)
                    .await?;
                slave.write_sdo(0x1a00, SubIndex::Index(0), 0x03u8).await?;

                slave.write_sdo(0x1c12, SubIndex::Index(0), 0u8).await?;
                slave.write_sdo(0x1c12, SubIndex::Index(1), 0x1600).await?;
                slave.write_sdo(0x1c12, SubIndex::Index(0), 1u8).await?;

                slave.write_sdo(0x1c13, SubIndex::Index(0), 0u8).await?;
                slave.write_sdo(0x1c13, SubIndex::Index(1), 0x1a00).await?;
                slave.write_sdo(0x1c13, SubIndex::Index(0), 1u8).await?;

                // Opmode - Cyclic Synchronous Position
                // slave.write_sdo(0x6060, SubIndex::Index(0), 0x08).await?;
                // Opmode - Cyclic Synchronous Velocity
                slave.write_sdo(0x6060, SubIndex::Index(0), 0x09u8).await?;
            }

            Ok(())
        })
    });

    let group = client
        .init::<16, _>(groups, |groups, slave| groups.push(slave))
        .await
        .expect("Init");

    client
        .request_slave_state(SlaveState::Op)
        .await
        .expect("OP");

    log::info!("Slaves moved to OP state");

    log::info!("Group has {} slaves", group.len());

    for slave in group.slaves() {
        let (i, o) = slave.io();

        log::info!(
            "-> Slave {} {} inputs: {} bytes, outputs: {} bytes",
            slave.configured_address,
            slave.name,
            i.len(),
            o.len(),
        );
    }

    // Run twice to prime PDI
    group.tx_rx(&client).await.expect("TX/RX");

    let cycle_time = {
        let slave = group.slave(0).unwrap();

        let base = slave
            .read_sdo::<u8>(&client, 0x60c2, SubIndex::Index(1))
            .await?;
        let x10 = slave
            .read_sdo::<i8>(&client, 0x60c2, SubIndex::Index(2))
            .await?;

        let base = f32::from(base);
        let x10 = 10.0f32.powi(i32::from(x10));

        let cycle_time_ms = (base * x10) * 1000.0;

        Duration::from_millis(unsafe { cycle_time_ms.round().to_int_unchecked() })
    };

    log::info!("Cycle time: {} ms", cycle_time.as_millis());

    // AKD will error with F706 if cycle time is not 2ms or less
    let mut cyclic_interval = Timer::interval(cycle_time);

    let slave = group.slave(0).expect("No servo!");
    let mut servo = Ds402Sm::new(Ds402::new(slave).expect("Failed to gather DS402"));

    let mut velocity: i32 = 0;

    // let mut slave = group.slave(0, &client).unwrap();

    let accel = 300;

    while let Some(_) = cyclic_interval.next().await {
        group.tx_rx(&client).await.expect("TX/RX");

        if servo.tick() {
            // // Opmode - Cyclic Synchronous Position
            // servo
            //     .sm
            //     .context()
            //     .slave
            //     .write_sdo(&client, 0x6060, SubIndex::Index(0), 0x08u8)
            //     .await?;

            let status = servo.status_word();
            let (i, o) = servo.slave().io();

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

            if running.load(Ordering::SeqCst) {
                if vel < 200_000 {
                    velocity += accel;
                }
            } else if vel > 0 {
                velocity -= accel;
            } else {
                break;
            }
        }
    }

    log::info!("Servo stopped, shutting drive down");

    while let Some(_) = cyclic_interval.next().await {
        group.tx_rx(&client).await.expect("TX/RX");

        if servo.tick_shutdown() {
            break;
        }

        let status = servo.status_word();
        let (i, o) = servo.slave().io();

        let (pos, vel) = {
            let pos = i32::from_le_bytes(i[2..=5].try_into().unwrap());
            let vel = i32::from_le_bytes(i[6..=9].try_into().unwrap());

            (pos, vel)
        };

        println!(
            "Position: {pos}, velocity: {vel}, status: {status:?} | {:?}",
            o
        );
    }

    Ok(())
}

fn main() -> Result<(), Error> {
    env_logger::init();
    let local_ex = LocalExecutor::new();

    futures_lite::future::block_on(local_ex.run(main_inner(&local_ex))).unwrap();

    Ok(())
}
