//! An experiment in how to safely represent and use the PDI (Process Data Image).
//!
//! At time of writing, requires EL2004, EL2004 and EL1004 in that order to function correctly due
//! to a pile of hard-coding.

use async_ctrlc::CtrlC;
use ethercrab::error::Error;
use ethercrab::std::tx_rx_task;
use ethercrab::Client;
use ethercrab::Pdi;
use ethercrab::SlaveState;
use futures_lite::stream::StreamExt;
use futures_lite::FutureExt;
use smol::LocalExecutor;;
use std::sync::Arc;
use std::time::Duration;

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

async fn main_inner(ex: &LocalExecutor<'static>) -> Result<(), Error> {
    let client = Arc::new(Client::<16, 16, 16, smol::Timer>::new());

    ex.spawn(tx_rx_task(INTERFACE, &client).unwrap()).detach();

    let num_slaves = client.num_slaves();

    client
        .init(|| async {
            println!("Nice");

            Ok(())
        })
        .await
        .expect("Init");

    log::info!("Discovered {num_slaves} slaves");

    // NOTE: Valid outputs must be provided before moving into operational state
    log::debug!("Moving slaves to OP...");

    match client.request_slave_state(SlaveState::Op).await {
        Ok(it) => it,
        Err(err) => {
            for idx in 0..num_slaves {
                let slave = client.slave_by_index(idx)?;

                let (status, code) = slave.status().await?;

                log::error!("Slave {idx} failed to transition to OP: {status:?} ({code})");
            }

            return Err(err);
        }
    };

    log::info!("Slaves moved to OP state");

    async_io::Timer::after(Duration::from_millis(100)).await;

    // TODO: Groups will have an offset address; use it instead of zero
    let mut group = Pdi::<16, 16>::new(0);

    let mut interval = async_io::Timer::interval(Duration::from_millis(50));

    while let Some(_) = interval.next().await {
        group.tx_rx(&client).await.unwrap();

        group.io(0).and_then(|(_i, o)| o).map(|o| {
            o[0] += 1;
        });

        let switches = group.io(2).and_then(|(i, _o)| i).map(|i| i[0]).unwrap();

        group.io(1).and_then(|(_i, o)| o).map(|o| {
            o[0] = switches;
        });
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
