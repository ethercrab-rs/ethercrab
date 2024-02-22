//! Configure Distributed Clocks (DC) for EK1100 and a couple of other modules.

use env_logger::Env;
use ethercrab::{
    error::Error, std::tx_rx_task, Client, ClientConfig, Command, PduStorage, RegisterAddress,
    Timeouts,
};
use std::{sync::Arc, time::Duration};
use tokio::time::MissedTickBehavior;

/// Maximum number of slaves that can be stored. This must be a power of 2 greater than 1.
const MAX_SLAVES: usize = 16;
const MAX_PDU_DATA: usize = 1100;
const MAX_FRAMES: usize = 16;
const PDI_LEN: usize = 64;

static PDU_STORAGE: PduStorage<MAX_FRAMES, MAX_PDU_DATA> = PduStorage::new();

const CYCLIC_OP_ENABLE: u8 = 0b0000_0001;
const SYNC0_ACTIVATE: u8 = 0b0000_0010;
const SYNC1_ACTIVATE: u8 = 0b0000_0100;

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
        Timeouts::default(),
        ClientConfig::default(),
    ));

    tokio::spawn(tx_rx_task(&interface, tx, rx).expect("spawn TX/RX task"));

    // The group will be in PRE-OP at this point
    let mut group = client
        .init_single_group::<MAX_SLAVES, PDI_LEN>()
        .await
        .expect("Init");

    for slave in group.iter(&client) {
        // Special configuration is required for some slave devices
        if slave.name() == "EL3004" {
            log::info!("Found EL3004. Configuring...");

            // Taken from TwinCAT
            slave.sdo_write(0x1c12, 0, 0u8).await?;
            slave.sdo_write(0x1c13, 0, 0u8).await?;

            slave.sdo_write(0x1c13, 1, 0x1a00u16).await?;
            slave.sdo_write(0x1c13, 2, 0x1a02u16).await?;
            slave.sdo_write(0x1c13, 3, 0x1a04u16).await?;
            slave.sdo_write(0x1c13, 4, 0x1a06u16).await?;
            slave.sdo_write(0x1c13, 0, 4u8).await?;
        } else if slave.name() == "LAN9252-EVB-HBI" {
            log::info!("Found LAN9252 in {:?} state", slave.status().await.ok());

            let sync_type = slave.sdo_read::<u16>(0x1c32, 1).await?;
            let cycle_time = slave.sdo_read::<u32>(0x1c32, 2).await?;
            log::info!("Outputs sync type {sync_type}, cycle time {cycle_time} ns");

            let sync_type = slave.sdo_read::<u16>(0x1c33, 1).await?;
            let cycle_time = slave.sdo_read::<u32>(0x1c33, 2).await?;
            log::info!("Inputs sync type {sync_type}, cycle time {cycle_time} ns");

            #[repr(u16)]
            enum Lan9252Register {
                /// 12.14.29 SYNC/LATCH PDI CONFIGURATION REGISTER.
                SyncLatchConfig = 0x0151,
            }

            #[derive(Debug, ethercrab_wire::EtherCrabWireRead)]
            #[repr(u8)]
            enum SyncLatchDrivePolarity {
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
            struct Lan9252Conf {
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

            let v = slave
                .register_read::<Lan9252Conf>(Lan9252Register::SyncLatchConfig as u16)
                .await
                .expect("0x0151u16");

            log::info!("LAN9252 config reg 0x0151: {:?}", v);

            // Table 27 – Distributed Clock sync parameter
            {
                // Disable cyclic op
                slave
                    .register_write(RegisterAddress::DcSyncActive, 0u8)
                    .await
                    .expect("DcSyncActive");

                // SOEM does it :shrug:. 0x0980 is just marked "reserved" in ETG1000.4.
                slave
                    .register_write(0x980u16, 0u8)
                    .await
                    .expect("DcSyncActive");

                let device_time = slave
                    .register_read::<i64>(RegisterAddress::DcSystemTime)
                    .await
                    .expect("DcSystemTime");

                log::info!("Device time {}", device_time);

                slave
                    .register_write(RegisterAddress::DcSyncStartTime, device_time + 10_000)
                    .await
                    .expect("DcSyncStartTime");

                // Cycle time in nanoseconds
                slave
                    .register_write(RegisterAddress::DcSync0CycleTime, 10_000)
                    .await
                    .expect("DcSync0CycleTime");

                // slave
                //     .register_write(RegisterAddress::DcSyncActive, SYNC0_ACTIVATE)
                //     .await
                //     .expect("DcSyncActive");
            }
        }
    }

    let mut group = group
        .into_safe_op(&client)
        .await
        .expect("PRE-OP -> SAFE-OP");

    log::info!("Group has {} slaves", group.len());

    for slave in group.iter(&client) {
        let (i, o) = slave.io_raw();

        log::info!(
            "-> Slave {:#06x} {} has inputs: {}, outputs: {}",
            slave.configured_address(),
            slave.name(),
            i.len(),
            o.len()
        );
    }

    let mut tick_interval = tokio::time::interval(Duration::from_millis(5));
    tick_interval.set_missed_tick_behavior(MissedTickBehavior::Skip);

    // Provide valid outputs before transition. LAN9252 will timeout going into OP if outputs are
    // not present.
    group.tx_rx(&client).await.expect("TX/RX");

    let mut group = group.into_op(&client).await.expect("SAFE-OP -> OP");

    // Enable cyclic op for all devices
    for slave in group.iter(&client) {
        // Enabling CYCLIC_OP_ENABLE gives "Failed to claim receiving frame" if not in OP state,
        // which is a bizarre error to encounter.
        slave
            .register_write(
                RegisterAddress::DcSyncActive,
                SYNC0_ACTIVATE | CYCLIC_OP_ENABLE,
            )
            .await
            .expect("DcSyncActive OP");
    }

    loop {
        group.tx_rx(&client).await.expect("TX/RX");

        // // Dynamic drift compensation
        // let _ = Command::frmw(0x1000, RegisterAddress::DcSystemTime.into())
        //     .wrap(&client)
        //     .with_wkc(group.len() as u16)
        //     .receive::<u64>()
        //     .await?;

        for mut slave in group.iter(&client) {
            let (_i, o) = slave.io_raw_mut();

            for byte in o.iter_mut() {
                *byte = byte.wrapping_add(1);
            }
        }

        tick_interval.tick().await;
    }
}
