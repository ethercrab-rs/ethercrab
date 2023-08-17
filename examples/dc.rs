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
            wait_loop_delay: Duration::from_millis(2),
            mailbox_response: Duration::from_millis(1000),
            ..Default::default()
        },
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
            let shift_time = slave.sdo_read::<u32>(0x1c32, 3).await.unwrap_or(0);
            log::info!("Outputs sync stuff {sync_type} {cycle_time} ns, shift {shift_time} ns");

            let sync_type = slave.sdo_read::<u16>(0x1c33, 1).await?;
            let cycle_time = slave.sdo_read::<u32>(0x1c33, 2).await?;
            let shift_time = slave.sdo_read::<u32>(0x1c33, 3).await.unwrap_or(0);
            log::info!("Inputs sync stuff {sync_type} {cycle_time} ns, shift {shift_time} ns");
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

    let mut group = group.into_op(&client).await.expect("SAFE-OP -> OP");

    let mut tick_interval = tokio::time::interval(Duration::from_millis(5));
    tick_interval.set_missed_tick_behavior(MissedTickBehavior::Skip);

    loop {
        group.tx_rx(&client).await.expect("TX/RX");

        // Dynamic drift compensation
        let (_reference_time, _wkc) = Command::frmw(0x1000, RegisterAddress::DcSystemTime.into())
            .receive::<u64>(&client)
            .await?;

        for slave in group.iter(&client) {
            let (_i, o) = slave.io_raw();

            for byte in o.iter_mut() {
                *byte = byte.wrapping_add(1);
            }
        }

        tick_interval.tick().await;
    }
}
