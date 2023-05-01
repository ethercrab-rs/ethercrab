//! Configure Distributed Clocks (DC) for EK1100 and a couple of other modules.

use env_logger::Env;
use ethercrab::{
    error::Error, std::tx_rx_task, Client, ClientConfig, PduStorage, RegisterAddress, SlaveGroup,
    SubIndex, Timeouts,
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

    let group = SlaveGroup::<MAX_SLAVES, PDI_LEN>::new(|slave| {
        Box::pin(async {
            // Special configuration is required for some slave devices
            if slave.name() == "EL3004" {
                log::info!("Found EL3004. Configuring...");

                // Taken from TwinCAT
                slave.write_sdo(0x1c12, SubIndex::Index(0), 0u8).await?;
                slave.write_sdo(0x1c13, SubIndex::Index(0), 0u8).await?;

                slave
                    .write_sdo(0x1c13, SubIndex::Index(1), 0x1a00u16)
                    .await?;
                slave
                    .write_sdo(0x1c13, SubIndex::Index(2), 0x1a02u16)
                    .await?;
                slave
                    .write_sdo(0x1c13, SubIndex::Index(3), 0x1a04u16)
                    .await?;
                slave
                    .write_sdo(0x1c13, SubIndex::Index(4), 0x1a06u16)
                    .await?;
                slave.write_sdo(0x1c13, SubIndex::Index(0), 4u8).await?;
            } else if slave.name() == "LAN9252-EVB-HBI" {
                log::info!("Found LAN9252 in {:?} state", slave.state().await.ok());

                let sync_type = slave.read_sdo::<u16>(0x1c32, SubIndex::Index(1)).await?;
                let cycle_time = slave.read_sdo::<u32>(0x1c32, SubIndex::Index(2)).await?;
                let shift_time = slave
                    .read_sdo::<u32>(0x1c32, SubIndex::Index(3))
                    .await
                    .unwrap_or(0);
                log::info!("Outputs sync stuff {sync_type} {cycle_time} ns, shift {shift_time} ns");

                let sync_type = slave.read_sdo::<u16>(0x1c33, SubIndex::Index(1)).await?;
                let cycle_time = slave.read_sdo::<u32>(0x1c33, SubIndex::Index(2)).await?;
                let shift_time = slave
                    .read_sdo::<u32>(0x1c33, SubIndex::Index(3))
                    .await
                    .unwrap_or(0);
                log::info!("Inputs sync stuff {sync_type} {cycle_time} ns, shift {shift_time} ns");
            }

            Ok(())
        })
    });

    let group = client
        // Initialise a single group
        .init::<1, _>(group, |group, _slave| Ok(group))
        .await
        .expect("Init");

    log::info!("Group has {} slaves", group.len());

    for slave in group.slaves() {
        let (i, o) = slave.io();

        log::info!(
            "-> Slave {} {} has inputs: {}, outputs: {}",
            slave.configured_address,
            slave.name,
            i.len(),
            o.len(),
        );
    }

    // NOTE: This is currently hardcoded as 2ms inside the DC sync config, so keep them the same.
    let mut tick_interval = tokio::time::interval(Duration::from_millis(5));
    tick_interval.set_missed_tick_behavior(MissedTickBehavior::Skip);

    let group = Arc::new(group);
    let group2 = group.clone();

    loop {
        group.tx_rx(&client).await.expect("TX/RX");

        // Dynamic drift compensation
        let (_reference_time, _wkc) = client
            .frmw::<u64>(0x1000, RegisterAddress::DcSystemTime)
            .await?;

        for slave in group2.slaves() {
            let (_i, o) = slave.io();

            for byte in o.iter_mut() {
                *byte = byte.wrapping_add(1);
            }
        }

        tick_interval.tick().await;
    }
}
