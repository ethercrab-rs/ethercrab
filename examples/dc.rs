//! Configure distributed clocks (DC) for a couple of slave devices.

use async_ctrlc::CtrlC;
use ethercrab::al_status::AlState;
use ethercrab::client::Client;
use ethercrab::error::PduError;
use ethercrab::fmmu::Fmmu;
use ethercrab::pdu::CheckWorkingCounter;
use ethercrab::register::RegisterAddress;
use ethercrab::std::tx_rx_task;
use futures_lite::FutureExt;
use futures_lite::StreamExt;
use smol::LocalExecutor;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

#[cfg(target_os = "windows")]
// ASRock NIC
// const INTERFACE: &str = "TODO";
// USB NIC
const INTERFACE: &str = "\\Device\\NPF_{DCEDC919-0A20-47A2-9788-FC57D0169EDB}";
// Silver USB NIC
// const INTERFACE: &str = "\\Device\\NPF_{CC0908D5-3CB8-46D6-B8A2-575D0578008D}";
#[cfg(not(target_os = "windows"))]
const INTERFACE: &str = "eth1";

async fn main_inner(ex: &LocalExecutor<'static>) -> Result<(), PduError> {
    let client = Arc::new(Client::<16, 16, 16, smol::Timer>::new());

    ex.spawn(tx_rx_task(INTERFACE, &client).unwrap()).detach();

    let (_res, num_slaves) = client.brd::<u8>(RegisterAddress::Type).await.unwrap();

    log::info!("Discovered {num_slaves} slaves");

    client.init().await.expect("Init");

    client
        .bwr(RegisterAddress::DcTimePort0, 0u32)
        .await
        .unwrap()
        .wkc(num_slaves, "write port 0 time")
        .unwrap();

    for i in 0..num_slaves {
        let delay: u32 = client
            .fprd(i, RegisterAddress::DcTimePort0)
            .await
            .unwrap()
            .wkc(1, "read port 0 time")
            .unwrap();

        println!("Slave {i} delay: {delay}");
    }

    Ok(())
}

fn main() -> Result<(), PduError> {
    env_logger::init();
    let local_ex = LocalExecutor::new();

    let ctrlc = CtrlC::new().expect("cannot create Ctrl+C handler?");

    futures_lite::future::block_on(
        local_ex.run(ctrlc.race(async { main_inner(&local_ex).await.unwrap() })),
    );

    Ok(())
}
