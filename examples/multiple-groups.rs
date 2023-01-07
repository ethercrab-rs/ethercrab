//! Demonstrate sorting slaves into multipel slave groups.
//!
//! This demo is designed to be used with the following slave devices:
//!
//! - EK1100
//! - EL2889 (2 bytes of outputs)
//! - EL2828 (1 byte of outputs)

use async_ctrlc::CtrlC;
use async_io::Timer;
use ethercrab::{
    error::Error, std::tx_rx_task, Client, PduLoop, PduStorage, SlaveGroup, SlaveGroupContainer,
    SlaveGroupRef, Timeouts,
};
use futures_lite::{FutureExt, StreamExt};
use smol::LocalExecutor;
use std::{sync::Arc, time::Duration};

/// Maximum number of slaves that can be stored.
const MAX_SLAVES: usize = 16;
/// Maximum PDU data payload size - set this to the max PDI size or higher.
const MAX_PDU_DATA: usize = 1100;
/// Maximum number of EtherCAT frames that can be in flight at any one time.
const MAX_FRAMES: usize = 16;

static PDU_STORAGE: PduStorage<MAX_FRAMES, MAX_PDU_DATA> = PduStorage::new();
static PDU_LOOP: PduLoop = PduLoop::new(PDU_STORAGE.as_ref());

#[derive(Default)]
struct Groups {
    /// EL2889 and EK1100. 2 items, 2 bytes of PDI for 16 output bits.
    ///
    /// We'll keep the EK1100 in here as it has no PDI but still needs to live somewhere.
    slow_outputs: SlaveGroup<2, 2, smol::Timer>,
    /// EL2828. 1 item, 1 byte of PDI for 8 output bits.
    fast_outputs: SlaveGroup<1, 1, smol::Timer>,
}

impl SlaveGroupContainer<smol::Timer> for Groups {
    fn num_groups(&self) -> usize {
        2
    }

    fn group(&mut self, index: usize) -> Option<SlaveGroupRef<smol::Timer>> {
        match index {
            0 => Some(self.slow_outputs.as_mut_ref()),
            1 => Some(self.fast_outputs.as_mut_ref()),
            _ => None,
        }
    }
}

async fn main_inner(ex: &LocalExecutor<'static>) -> Result<(), Error> {
    let interface = std::env::args()
        .nth(1)
        .expect("Provide interface as first argument");

    log::info!("Starting multiple groups demo...");

    let client = Arc::new(Client::<smol::Timer>::new(
        &PDU_LOOP,
        Timeouts {
            wait_loop_delay: Duration::from_millis(2),
            mailbox_response: Duration::from_millis(1000),
            ..Default::default()
        },
    ));

    ex.spawn(tx_rx_task(&interface, &client).unwrap()).detach();

    let groups = client
        // Initialise up to 16 slave devices
        .init::<MAX_SLAVES, _>(Groups::default(), |groups, slave| {
            match slave.name.as_str() {
                "EL2889" | "EK1100" => groups.slow_outputs.push(slave),
                "EL2828" => groups.fast_outputs.push(slave),
                _ => Err(Error::UnknownSlave),
            }
        })
        .await
        .expect("Init");

    let Groups {
        slow_outputs,
        fast_outputs,
    } = groups;

    let client_slow = client.clone();

    let slow_task = smol::spawn(async move {
        let mut slow_tick_interval = Timer::interval(Duration::from_millis(250));

        while let Some(_) = slow_tick_interval.next().await {
            slow_outputs.tx_rx(&client_slow).await.expect("TX/RX");

            // Increment every output byte for every slave device by one
            for slave in slow_outputs.slaves() {
                let (_i, o) = slave.io();

                for byte in o.iter_mut() {
                    *byte = byte.wrapping_add(1);
                }
            }
        }
    });

    let fast_task = smol::spawn(async move {
        let mut fast_tick_interval = Timer::interval(Duration::from_millis(5));

        while let Some(_) = fast_tick_interval.next().await {
            fast_outputs.tx_rx(&client).await.expect("TX/RX");

            // Increment every output byte for every slave device by one
            for slave in fast_outputs.slaves() {
                let (_i, o) = slave.io();

                for byte in o.iter_mut() {
                    *byte = byte.wrapping_add(1);
                }
            }
        }
    });

    futures_lite::future::race(slow_task, fast_task).await;

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
