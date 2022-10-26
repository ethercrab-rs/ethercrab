//! Configure Distributed Clocks (DC) for EK1100 and a couple of other modules.

use async_ctrlc::CtrlC;
use async_io::Timer;
use chrono::{TimeZone, Utc};
use ethercrab::{
    error::Error,
    register::{PortDescriptors, RegisterAddress, SupportFlags},
    std::tx_rx_task,
    CheckWorkingCounter, Client, PduLoop, SlaveGroup, Timeouts,
};
use futures_lite::FutureExt;
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
    log::info!("Starting DC demo...");

    let client = Arc::new(Client::<MAX_FRAMES, MAX_PDU_DATA, smol::Timer>::new(
        &PDU_LOOP,
        Timeouts {
            wait_loop_delay: Duration::from_millis(5),
            mailbox: Duration::from_millis(50),
            ..Default::default()
        },
    ));

    ex.spawn(tx_rx_task(INTERFACE, &client).unwrap()).detach();

    let group = SlaveGroup::<MAX_SLAVES, PDI_LEN, MAX_FRAMES, MAX_PDU_DATA, _>::new(|_slave| {
        Box::pin(async { Ok(()) })
    });

    let group = client
        .init::<16, _>(group, |groups, slave| groups.push(slave))
        .await
        .expect("Init");

    // {
    //     // TODO: Read from slave list flags.dc_supported
    //     let first_dc_supported_slave = 0x1000;

    //     client
    //         .bwr(RegisterAddress::DcTimePort0, 0u32)
    //         .await
    //         .expect("Broadcast time")
    //         .wkc(group.slaves().len() as u16, "Broadcast time")
    //         .unwrap();

    //     Timer::after(Duration::from_millis(100)).await;

    //     let ethercat_offset = Utc.ymd(2000, 01, 01).and_hms(0, 0, 0);

    //     let now_nanos =
    //         chrono::Utc::now().timestamp_nanos() - dbg!(ethercat_offset.timestamp_nanos());

    //     for slave in group.slaves() {
    //         // let port_descriptors = client
    //         //     .fprd::<PortDescriptors>(slave.configured_address, RegisterAddress::PortDescriptors)
    //         //     .await
    //         //     .expect("Supported flags")
    //         //     .wkc(1, "Supported flags")
    //         //     .unwrap();

    //         // log::info!(
    //         //     "Slave {:#06x} ports: {:#?}",
    //         //     slave.configured_address,
    //         //     port_descriptors
    //         // );

    //         let flags = client
    //             .fprd::<SupportFlags>(slave.configured_address, RegisterAddress::SupportFlags)
    //             .await
    //             .expect("Supported flags")
    //             .wkc(1, "Supported flags")
    //             .unwrap();

    //         let time_p0 = client
    //             .fprd::<u32>(slave.configured_address, RegisterAddress::DcTimePort0)
    //             .await
    //             .expect("DC time port 0")
    //             .wkc(1, "DC time port 0")
    //             .unwrap();

    //         let receive_time_p0_nanos = client
    //             .fprd::<i64>(slave.configured_address, RegisterAddress::DcReceiveTime)
    //             .await
    //             .expect("Receive time P0")
    //             .wkc(1, "Receive time P0")
    //             .unwrap();

    //         // let offset = u64::try_from(now_nanos).expect("Why negative???") - receive_time_p0;
    //         let offset = u64::try_from(-receive_time_p0_nanos + now_nanos).unwrap();

    //         dbg!(offset);

    //         // Final slave returns a WKC of 0 for some reason
    //         client
    //             .fpwr::<u64>(
    //                 slave.configured_address,
    //                 RegisterAddress::DcSystemTimeOffset,
    //                 offset,
    //             )
    //             .await?
    //             .wkc(1, "Write offset")
    //             .expect("Write offset");

    //         let time_p1 = client
    //             .fprd::<u32>(slave.configured_address, RegisterAddress::DcTimePort1)
    //             .await
    //             .expect("DC time port 1")
    //             .wkc(1, "DC time port 1")
    //             .unwrap();

    //         let time_p2 = client
    //             .fprd::<u32>(slave.configured_address, RegisterAddress::DcTimePort2)
    //             .await
    //             .expect("DC time port 2")
    //             .wkc(1, "DC time port 2")
    //             .unwrap();

    //         let time_p3 = client
    //             .fprd::<u32>(slave.configured_address, RegisterAddress::DcTimePort3)
    //             .await
    //             .expect("DC time port 3")
    //             .wkc(1, "DC time port 3")
    //             .unwrap();

    //         log::info!(
    //             "Slave {:#06x} times: ({}, {}, {}, {})",
    //             slave.configured_address,
    //             time_p0,
    //             time_p1,
    //             time_p2,
    //             time_p3
    //         );

    //         log::info!(
    //             "Slave {:#06x} receive time: {} ns",
    //             slave.configured_address,
    //             receive_time_p0_nanos
    //         );

    //         if !flags.has_64bit_dc {
    //             // TODO
    //             log::warn!("Slave uses seconds instead of ns?");
    //         }

    //         if !flags.dc_supported {
    //             continue;
    //         }
    //     }
    // }

    // client
    //     .request_slave_state(SlaveState::Op)
    //     .await
    //     .expect("OP");

    // log::info!("Slaves moved to OP state");

    log::info!("Group has {} slaves", group.slaves().len());

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
