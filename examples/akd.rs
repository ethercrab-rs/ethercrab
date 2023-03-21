//! Configure a Kollmorgen AKD servo drive and put it in enabled state.

use env_logger::Env;
use ethercrab::{
    error::{Error, MailboxError},
    std::tx_rx_task,
    Client, ClientConfig, PduStorage, SlaveGroup, SlaveState, SubIndex, Timeouts,
};
use std::{sync::Arc, time::Duration};
use tokio::time::MissedTickBehavior;

const MAX_SLAVES: usize = 16;
const MAX_PDU_DATA: usize = 1100;
const MAX_FRAMES: usize = 16;
const PDI_LEN: usize = 64;

static PDU_STORAGE: PduStorage<MAX_FRAMES, MAX_PDU_DATA> = PduStorage::new();

#[tokio::main]
async fn main() -> Result<(), Error> {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    let interface = std::env::args()
        .nth(1)
        .expect("Provide interface as first argument. Pass an unrecognised name to list available interfaces.");

    log::info!("Starting AKD demo...");
    log::info!("Ensure a Kollmorgen AKD drive is the first slave device");
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

            let profile = match slave.read_sdo::<u32>(0x1000, SubIndex::Index(0)).await {
                Err(Error::Mailbox(MailboxError::NoMailbox)) => Ok(None),
                Ok(device_type) => Ok(Some(device_type & 0xffff)),
                Err(e) => Err(e),
            }?;

            // CiA 402/DS402 device
            if profile == Some(402) {
                log::info!("Slave {} supports DS402", slave.name());
            }

            // AKD config
            if slave.name() == "AKD" {
                slave.write_sdo(0x1c12, SubIndex::Index(0), 0u8).await?;
                // 0x1702 = fixed velocity mapping
                slave
                    .write_sdo(0x1c12, SubIndex::Index(1), 0x1702u16)
                    .await?;
                slave.write_sdo(0x1c12, SubIndex::Index(0), 0x01u8).await?;

                // Must set both read AND write SDOs for AKD otherwise it times out going into OP
                slave.write_sdo(0x1c13, SubIndex::Index(0), 0u8).await?;
                slave
                    .write_sdo(0x1c13, SubIndex::Index(1), 0x1B01u16)
                    .await?;
                slave.write_sdo(0x1c13, SubIndex::Index(0), 0x01u8).await?;

                // Opmode - Cyclic Synchronous Position
                // slave.write_sdo(0x6060, SubIndex::Index(0), 0x08).await?;
                // Opmode - Cyclic Synchronous Velocity
                slave.write_sdo(0x6060, SubIndex::Index(0), 0x09u8).await?;

                {
                    // Shows up as default value of 2^20, but I'm using a 2^32 counts/rev encoder.
                    let encoder_increments =
                        slave.read_sdo::<u32>(0x608f, SubIndex::Index(1)).await?;
                    let num_revs = slave.read_sdo::<u32>(0x608f, SubIndex::Index(2)).await?;

                    let gear_ratio_motor =
                        slave.read_sdo::<u32>(0x6091, SubIndex::Index(1)).await?;
                    let gear_ratio_final =
                        slave.read_sdo::<u32>(0x6091, SubIndex::Index(2)).await?;

                    let feed = slave.read_sdo::<u32>(0x6092, SubIndex::Index(1)).await?;
                    let shaft_revolutions =
                        slave.read_sdo::<u32>(0x6092, SubIndex::Index(2)).await?;

                    let counts_per_rev = encoder_increments / num_revs;

                    log::info!("Drive info");
                    log::info!("--> Encoder increments     {}", encoder_increments);
                    log::info!("--> Number of revolutions  {}", num_revs);
                    log::info!("--> Gear ratio (motor)     {}", gear_ratio_motor);
                    log::info!("--> Gear ratio (final)     {}", gear_ratio_final);
                    log::info!("--> Feed                   {}", feed);
                    log::info!("--> Shaft revolutions      {}", shaft_revolutions);
                    log::info!("--> Counts per rev         {}", counts_per_rev);
                }
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

    log::info!("Discovered {} slaves", group.len());

    let slave = group.slave(0).expect("first slave not found");

    // Run twice to prime PDI
    group.tx_rx(&client).await.expect("TX/RX");

    let cycle_time = {
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

    // AKD will error with F706 if cycle time is not 2ms or less, but we're reading it from the
    // drive so we should be fine.
    let mut cyclic_interval = tokio::time::interval(cycle_time);
    cyclic_interval.set_missed_tick_behavior(MissedTickBehavior::Skip);

    // Check for and clear faults
    {
        log::info!("Checking faults");

        group.tx_rx(&client).await.expect("TX/RX");

        let (i, o) = slave.io();

        let status = {
            let status = u16::from_le_bytes(i[4..=5].try_into().unwrap());

            AkdStatusWord::from_bits_truncate(status)
        };

        if status.contains(AkdStatusWord::FAULT) {
            log::warn!("Fault! Clearing...");

            let (_pos_cmd, control) = o.split_at_mut(4);
            let reset = AkdControlWord::RESET_FAULT;
            let reset = reset.bits().to_le_bytes();
            control.copy_from_slice(&reset);

            loop {
                group.tx_rx(&client).await.expect("TX/RX");

                let (i, _o) = slave.io();

                let status = {
                    let status = u16::from_le_bytes(i[4..=5].try_into().unwrap());

                    AkdStatusWord::from_bits_truncate(status)
                };

                if !status.contains(AkdStatusWord::FAULT) {
                    log::info!("Fault cleared, status is now {status:?}");

                    break;
                }

                cyclic_interval.tick().await;
            }
        }
    }

    // Shutdown state
    {
        log::info!("Putting drive in shutdown state");

        let (_i, o) = slave.io();

        let (_pos_cmd, control) = o.split_at_mut(4);
        let value = AkdControlWord::SHUTDOWN;
        let value = value.bits().to_le_bytes();
        control.copy_from_slice(&value);

        loop {
            group.tx_rx(&client).await.expect("TX/RX");

            let (i, _o) = slave.io();

            let status = {
                let status = u16::from_le_bytes(i[4..=5].try_into().unwrap());

                AkdStatusWord::from_bits_truncate(status)
            };

            if status.contains(AkdStatusWord::READY_TO_SWITCH_ON) {
                log::info!("Drive is shut down");

                break;
            }

            cyclic_interval.tick().await;
        }
    }

    // Switch drive on
    {
        log::info!("Switching drive on");

        let (_i, o) = slave.io();

        let (_pos_cmd, control) = o.split_at_mut(4);
        let reset = AkdControlWord::SWITCH_ON
            | AkdControlWord::DISABLE_VOLTAGE
            | AkdControlWord::QUICK_STOP;
        let reset = reset.bits().to_le_bytes();
        control.copy_from_slice(&reset);

        loop {
            group.tx_rx(&client).await.expect("TX/RX");

            let (i, o) = slave.io();

            let status = {
                let status = u16::from_le_bytes(i[4..=5].try_into().unwrap());

                AkdStatusWord::from_bits_truncate(status)
            };

            if status.contains(AkdStatusWord::SWITCHED_ON) {
                log::info!("Drive switched on, begin cyclic operation");

                let (_pos_cmd, control) = o.split_at_mut(4);

                // Enable operation so we can send cyclic data
                let state = AkdControlWord::SWITCH_ON
                    | AkdControlWord::DISABLE_VOLTAGE
                    | AkdControlWord::QUICK_STOP
                    | AkdControlWord::ENABLE_OP;
                let state = state.bits().to_le_bytes();
                control.copy_from_slice(&state);

                break;
            }

            cyclic_interval.tick().await;
        }
    }

    let mut velocity: i32 = 0;

    loop {
        group.tx_rx(&client).await.expect("TX/RX");

        let (i, o) = slave.io();

        let (pos, status) = {
            let pos = u32::from_le_bytes(i[0..=3].try_into().unwrap());
            let status = u16::from_le_bytes(i[4..=5].try_into().unwrap());

            let status = AkdStatusWord::from_bits_truncate(status);

            (pos, status)
        };

        println!("Position: {pos}, Status: {status:?} | {:?}", o);

        let (pos_cmd, _control) = o.split_at_mut(4);

        pos_cmd.copy_from_slice(&velocity.to_le_bytes());

        if velocity < 100_000_0 {
            velocity += 200;
        }

        cyclic_interval.tick().await;
    }
}

bitflags::bitflags! {
    /// AKD EtherCAT Communications Manual section 5.3.55
    struct AkdControlWord: u16 {
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

        const SHUTDOWN = Self::DISABLE_VOLTAGE.bits() | Self::QUICK_STOP.bits();
    }
}

bitflags::bitflags! {
    /// AKD EtherCAT Communications Manual section   5.3.56
    #[derive(Debug)]
    struct AkdStatusWord: u16 {
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
