//! Read all detexted device names using SII.

use async_ctrlc::CtrlC;
use ethercrab::al_status::AlState;
use ethercrab::client::Client;
use ethercrab::error::PduError;
use ethercrab::register::RegisterAddress;
use ethercrab::std::tx_rx_task;
use futures_lite::FutureExt;
use smol::LocalExecutor;
use std::sync::Arc;

#[cfg(target_os = "windows")]
// ASRock NIC
// const INTERFACE: &str = "TODO";
// USB NIC
const INTERFACE: &str = "\\Device\\NPF_{DCEDC919-0A20-47A2-9788-FC57D0169EDB}";
// // Silver USB NIC
// const INTERFACE: &str = "\\Device\\NPF_{DCEDC919-0A20-47A2-9788-FC57D0169EDB}";
#[cfg(not(target_os = "windows"))]
const INTERFACE: &str = "eth1";

fn main() -> Result<(), PduError> {
    env_logger::init();
    let local_ex = LocalExecutor::new();

    let ctrlc = CtrlC::new().expect("cannot create Ctrl+C handler?");

    futures_lite::future::block_on(local_ex.run(ctrlc.race(async {
        let client = Arc::new(Client::<16, 16, 16, smol::Timer>::new());

        local_ex
            .spawn(tx_rx_task(INTERFACE, &client).unwrap())
            .detach();

        let (_res, num_slaves) = client.brd::<u8>(RegisterAddress::Type).await.unwrap();

        println!("Discovered {num_slaves} slaves");

        client.init().await.expect("Init");

        for slave_idx in 0..num_slaves {
            client
                .request_slave_state(AlState::Init)
                .await
                .expect("INIT");

            let slave = client.slave_by_index(slave_idx).expect("Slave");

            let name = slave.eeprom().device_name::<64>().await.expect("Read name");

            log::info!("Slave #{slave_idx} name: {name:?}");

            let sm = slave.eeprom().sync_managers().await.expect("SM load");

            dbg!(sm);
        }
    })));

    Ok(())
}
