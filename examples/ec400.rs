//! Configure a Leadshine EtherCat EL7 series drive and turn the motor.

use async_ctrlc::CtrlC;
use async_io::Timer;
use ethercrab::{
    error::Error, std::tx_rx_task, Client, PduLoop, SlaveGroup, SlaveState, SubIndex, Timeouts,
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

static PDU_LOOP: PduLoop<MAX_FRAMES, MAX_PDU_DATA, smol::Timer> = PduLoop::new();

async fn main_inner(ex: &LocalExecutor<'static>) -> Result<(), Error> {
    log::info!("Starting SDO demo...");

    let client = Arc::new(Client::<MAX_FRAMES, MAX_PDU_DATA, smol::Timer>::new(
        &PDU_LOOP,
        Timeouts::default(),
    ));

    ex.spawn(tx_rx_task(INTERFACE, &client).unwrap()).detach();

    // let num_slaves = client.num_slaves();

    let groups = SlaveGroup::<MAX_SLAVES, PDI_LEN, MAX_FRAMES, MAX_PDU_DATA, _>::new(|slave| {
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
                slave.write_sdo(0x1c12, SubIndex::Index(0), 0u8).await?;
                slave
                    .write_sdo(0x1c12, SubIndex::Index(1), 0x1601u16)
                    .await?;
                slave.write_sdo(0x1c12, SubIndex::Index(0), 0x01u8).await?;

                slave.write_sdo(0x1c13, SubIndex::Index(0), 0u8).await?;
                slave
                    .write_sdo(0x1c13, SubIndex::Index(1), 0x1A00u16)
                    .await?;
                slave.write_sdo(0x1c13, SubIndex::Index(0), 0x01u8).await?;
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
        let (i, o) = group.io(slave).unwrap();

        log::info!(
            "-> Slave {slave} {} has inputs: {}, outputs: {}",
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

    log::debug!("Cycle time: {} ms", cycle_time.as_millis());

    // AKD will error with F706 if cycle time is not 2ms or less
    let mut cyclic_interval = Timer::interval(cycle_time);

    // Check for and clear faults
    {
        log::info!("Checking faults");

        group.tx_rx(&client).await.expect("TX/RX");

        let (i, o) = group.io(0).unwrap();

        let status = i
            .map(|i| {
                let status = u16::from_le_bytes(i[4..=5].try_into().unwrap());

                unsafe { StatusWord::from_bits_unchecked(status) }
            })
            .unwrap();

        if status.contains(StatusWord::FAULT) {
            log::warn!("Fault! Clearing...");

            o.map(|o| {
                let (_pos_cmd, control) = o.split_at_mut(4);
                let reset = ControlWord::RESET_FAULT;
                let reset = reset.bits().to_le_bytes();
                control.copy_from_slice(&reset);
            });

            while let Some(_) = cyclic_interval.next().await {
                group.tx_rx(&client).await.expect("TX/RX");

                let (i, _o) = group.io(0).unwrap();

                let status = i
                    .map(|i| {
                        let status = u16::from_le_bytes(i[4..=5].try_into().unwrap());

                        unsafe { StatusWord::from_bits_unchecked(status) }
                    })
                    .unwrap();

                if !status.contains(StatusWord::FAULT) {
                    log::info!("Fault cleared, status is now {status:?}");

                    break;
                }
            }
        }
    }

    // Shutdown state
    {
        log::info!("Putting drive in shutdown state");

        let (_i, o) = group.io(0).unwrap();

        o.map(|o| {
            let (_pos_cmd, control) = o.split_at_mut(4);
            let value = ControlWord::SHUTDOWN;
            let value = value.bits().to_le_bytes();
            control.copy_from_slice(&value);
        });

        while let Some(_) = cyclic_interval.next().await {
            group.tx_rx(&client).await.expect("TX/RX");

            let (i, _o) = group.io(0).unwrap();

            let status = i
                .map(|i| {
                    let status = u16::from_le_bytes(i[4..=5].try_into().unwrap());

                    unsafe { StatusWord::from_bits_unchecked(status) }
                })
                .unwrap();

            if status.contains(StatusWord::READY_TO_SWITCH_ON) {
                log::info!("Drive is shut down");

                break;
            }
        }
    }

    // Switch drive on
    {
        log::info!("Switching drive on");

        let (_i, o) = group.io(0).unwrap();

        o.map(|o| {
            let (_pos_cmd, control) = o.split_at_mut(4);
            let reset =
                ControlWord::SWITCH_ON | ControlWord::DISABLE_VOLTAGE | ControlWord::QUICK_STOP;
            let reset = reset.bits().to_le_bytes();
            control.copy_from_slice(&reset);
        });

        while let Some(_) = cyclic_interval.next().await {
            group.tx_rx(&client).await.expect("TX/RX");

            let (i, o) = group.io(0).unwrap();

            let status = i
                .map(|i| {
                    let status = u16::from_le_bytes(i[4..=5].try_into().unwrap());

                    unsafe { StatusWord::from_bits_unchecked(status) }
                })
                .unwrap();

            if status.contains(StatusWord::SWITCHED_ON) {
                log::info!("Drive switched on, begin cyclic operation");

                o.map(|o| {
                    let (_pos_cmd, control) = o.split_at_mut(4);

                    // Enable operation so we can send cyclic data
                    let state = ControlWord::SWITCH_ON
                        | ControlWord::DISABLE_VOLTAGE
                        | ControlWord::QUICK_STOP
                        | ControlWord::ENABLE_OP;
                    let state = state.bits().to_le_bytes();
                    control.copy_from_slice(&state);
                });

                break;
            }
        }
    }

    smol::spawn(async move {
        let mut velocity: i32 = 0;

        while let Some(_) = cyclic_interval.next().await {
            group.tx_rx(&client).await.expect("TX/RX");

            let (i, o) = group.io(0).unwrap();

            let (pos, status) = i
                .map(|i| {
                    let pos = u32::from_le_bytes(i[0..=3].try_into().unwrap());
                    let status = u16::from_le_bytes(i[4..=5].try_into().unwrap());

                    let status = unsafe { StatusWord::from_bits_unchecked(status) };

                    (pos, status)
                })
                .unwrap();

            println!(
                "Position: {pos}, Status: {status:?} | {:?}",
                o.as_ref().unwrap()
            );

            o.map(|o| {
                let (pos_cmd, _control) = o.split_at_mut(4);

                pos_cmd.copy_from_slice(&velocity.to_le_bytes());
            });

            if velocity < 100_000_0 {
                velocity += 200;
            }
        }
    })
    .await;

    Ok(())
}

bitflags::bitflags! {
    /// AKD EtherCAT Communications Manual section 5.3.55
    struct ControlWord: u16 {
        /// Switch on
        const SWITCH_ON = 1 << 0;
        /// Disable Voltage
        const DISABLE_VOLTAGE = 1 << 1;
        /// Quick Stop
        const QUICK_STOP = 1 << 2;
        /// Enable Operation
        const ENABLE_OP = 1 << 3;
        /// Operation mode specific
        const OP_SPECIFIC_1 = 1 << 4;
        /// Operation mode specific
        const OP_SPECIFIC_2 = 1 << 5;
        /// Operation mode specific
        const OP_SPECIFIC_3 = 1 << 6;
        /// Reset Fault (only effective for faults)
        const RESET_FAULT = 1 << 7;
        /// Pause/halt
        const PAUSE = 1 << 8;

        const SHUTDOWN = Self::DISABLE_VOLTAGE.bits | Self::QUICK_STOP.bits;
    }
}

bitflags::bitflags! {
    /// AKD EtherCAT Communications Manual section   5.3.56
    struct StatusWord: u16 {
        /// Ready to switch on
        const READY_TO_SWITCH_ON = 1 << 0;
        /// Switched on
        const SWITCHED_ON = 1 << 1;
        /// Operation enabled
        const OP_ENABLED = 1 << 2;
        /// Fault
        const FAULT = 1 << 3;
        /// Voltage enabled
        const VOLTAGE_ENABLED = 1 << 4;
        /// Quick stop
        const QUICK_STOP = 1 << 5;
        /// Switch on disabled
        const SWITCH_ON_DISABLED = 1 << 6;
        /// Warning
        const WARNING = 1 << 7;
        /// STO – Safe Torque Off
        const STO = 1 << 8;
        /// Remote
        const REMOTE = 1 << 9;
        /// Target reached
        const TARGET_REACHED = 1 << 10;
        /// Internal limit active
        const INTERNAL_LIMIT = 1 << 11;
        /// Operation mode specific (reserved)
        const OP_SPECIFIC_1 = 1 << 12;
        /// Operation mode specific (reserved)
        const OP_SPECIFIC_2 = 1 << 13;
        /// Manufacturer-specific (reserved)
        const MAN_SPECIFIC_1 = 1 << 14;
        /// Manufacturer-specific (reserved)
        const MAN_SPECIFIC_2 = 1 << 15;
    }
}

#[derive(Debug, packed_struct::PackedStruct)]
struct Inputs {
    last_error: u16,
    status: u16,
    op_mode_display: u8,
    actual_position: u32,
    probe: u16,
    probe_2: u32,
    digital_inputs: u32,
}

#[derive(Debug, packed_struct::PackedStruct)]
struct Outputs {
    control_word: u16,
    target_position: u32,
    probe: u16,
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
