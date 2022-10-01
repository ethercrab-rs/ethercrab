//! Read slave configuration from EEPROM and automatically apply it.
//!
//! PDI is hardcoded to two bytes to support 2x EL2004 output modules and 1x EL1004 input module in
//! that order.

use async_ctrlc::CtrlC;
use ethercrab::error::Error;
use ethercrab::std::tx_rx_task;
use ethercrab::Client;
use ethercrab::SlaveState;
use futures_lite::stream::StreamExt;
use futures_lite::FutureExt;
use smol::LocalExecutor;
use std::cell::RefCell;
use std::rc::Rc;
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

    log::info!("Discovered {num_slaves} slaves");

    client.init().await.expect("Init");

    let value = Rc::new(RefCell::new([0u8; 8]));
    let value2 = value.clone();
    let client2 = client.clone();

    // Cyclic data task
    ex.spawn(async move {
        // Cycle time
        let mut interval = async_io::Timer::interval(Duration::from_millis(2));

        while let Some(_) = interval.next().await {
            let v = value2.try_borrow_mut();

            if let Ok(mut v) = v {
                let (res, wkc) = client2.lrw(0u32, *v).await.expect("Bad write");

                *v = res;

                assert!(wkc > 0, "main loop wkc");
            }
        }
    })
    .detach();

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

    // // RX-only PDI, second byte contains 4 input state bits
    // {
    //     let value = Rc::new(RefCell::new([0u8; 2]));

    //     let value2 = value.clone();
    //     let client2 = client.clone();

    //     ex.spawn(async move {
    //         // Cycle time
    //         let mut interval = async_io::Timer::interval(Duration::from_millis(2));

    //         while let Some(_) = interval.next().await {
    //             let v = *value2.borrow();

    //             let read = client2.lrw(0u32, v).await.expect("Bad write");

    //             dbg!(read.0);
    //         }
    //     })
    //     .await;
    // }

    // Blink frequency
    let mut interval = async_io::Timer::interval(Duration::from_millis(50));

    while let Some(_) = interval.next().await {
        let v = value.try_borrow_mut();

        if let Ok(mut v) = v {
            v[0] += 1;
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
