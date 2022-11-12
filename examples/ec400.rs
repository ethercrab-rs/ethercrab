//! Configure a Leadshine EtherCat EL7 series drive and turn the motor.

use async_ctrlc::CtrlC;
use async_io::Timer;
use ethercrab::{
    ds402::{Ds402, Ds402Sm},
    error::Error,
    std::tx_rx_task,
    Client, GroupSlave, PduLoop, PduStorage, SlaveGroup, SlaveState, SubIndex, Timeouts,
    TimerFactory,
};
use futures_lite::{FutureExt, StreamExt};
use smol::LocalExecutor;
use std::{sync::Arc, time::Duration};

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
static PDU_LOOP: PduLoop = PduLoop::new(PDU_STORAGE.as_ref());

async fn main_inner(ex: &LocalExecutor<'static>) -> Result<(), Error> {
    log::info!("Starting SDO demo...");

    let client = Arc::new(Client::<smol::Timer>::new(&PDU_LOOP, Timeouts::default()));

    ex.spawn(tx_rx_task(INTERFACE, &client).unwrap()).detach();

    // let num_slaves = client.num_slaves();

    let groups = SlaveGroup::<MAX_SLAVES, PDI_LEN, _>::new(|slave| {
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

    log::info!("Group has {} slaves", group.slaves().len());

    for (slave, slave_stuff) in group.slaves().iter().enumerate() {
        let sl = group.slave(slave, &client).unwrap();
        let (i, o) = (sl.inputs, sl.outputs);

        log::info!(
            "-> Slave {slave} {} inputs: {} bytes, outputs: {} bytes",
            slave_stuff.name,
            i.map(|stuff| stuff.len()).unwrap_or(0),
            o.map(|stuff| stuff.len()).unwrap_or(0)
        );
    }

    // Run twice to prime PDI
    group.tx_rx(&client).await.expect("TX/RX");

    let cycle_time = {
        let slave = group.slave(0, &client).unwrap();

        let base = slave.read_sdo::<u8>(0x60c2, SubIndex::Index(1)).await?;
        let x10 = slave.read_sdo::<i8>(0x60c2, SubIndex::Index(2)).await?;

        let base = f32::from(base);
        let x10 = 10.0f32.powi(i32::from(x10));

        let cycle_time_ms = (base * x10) * 1000.0;

        Duration::from_millis(unsafe { cycle_time_ms.round().to_int_unchecked() })
    };

    log::info!("Cycle time: {} ms", cycle_time.as_millis());

    // AKD will error with F706 if cycle time is not 2ms or less
    let mut cyclic_interval = Timer::interval(cycle_time);

    let mut slave = group.slave(0, &client).expect("No servo!");
    let mut servo = Ds402Sm::new(Ds402::new(&mut slave).expect("Failed to gather DS402"));

    let mut velocity: i32 = 0;

    // let mut slave = group.slave(0, &client).unwrap();

    while let Some(_) = cyclic_interval.next().await {
        group.tx_rx(&client).await.expect("TX/RX");

        if servo.tick() {
            let status = servo.status_word();
            let (i, o) = servo.io();

            let (pos, vel) = {
                let pos = u32::from_le_bytes(i[2..=5].try_into().unwrap());
                let vel = u32::from_le_bytes(i[6..=9].try_into().unwrap());

                (pos, vel)
            };

            println!(
                "Position: {pos}, velocity: {vel}, status: {status:?} | {:?}",
                o
            );

            let pos_cmd = &mut o[2..=5];

            pos_cmd.copy_from_slice(&velocity.to_le_bytes());

            if velocity < 200_000 {
                velocity += 200;
            }
        }
    }

    Ok(())
}

fn main() -> Result<(), Error> {
    env_logger::init();
    let local_ex = LocalExecutor::new();

    let ctrlc = CtrlC::new().expect("cannot create Ctrl+C handler?");

    futures_lite::future::block_on(
        local_ex.run(ctrlc.race(async { main_inner(&local_ex).await.unwrap() })),
    );

    Ok(())
}
