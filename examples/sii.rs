//! Set slave addresses using `client.init()` and request pre-operational state for both slaves.
//!
//! This is designed for use with the EK1100 + EL2004.

use async_ctrlc::CtrlC;
use ethercrab::al_status::AlState;
use ethercrab::client::Client;
use ethercrab::error::PduError;
use ethercrab::mailbox::MailboxProtocols;
use ethercrab::register::RegisterAddress;
use ethercrab::sii::SiiCoding;
use ethercrab::std::tx_rx_task;
use futures_lite::FutureExt;
use smol::LocalExecutor;
use std::sync::Arc;

#[cfg(target_os = "windows")]
// ASRock NIC
// const INTERFACE: &str = "TODO";
// USB NIC
// const INTERFACE: &str = "\\Device\\NPF_{DCEDC919-0A20-47A2-9788-FC57D0169EDB}";
// Silver USB NIC
const INTERFACE: &str = "\\Device\\NPF_{CC0908D5-3CB8-46D6-B8A2-575D0578008D}";
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

        client
            .request_slave_state(AlState::PreOp)
            .await
            .expect("Pre-op");

        for slave_idx in 0..num_slaves {
            let slave = client.slave_by_index(slave_idx).expect("Slave");

            let vendor_id = slave.read_eeprom_raw(SiiCoding::VendorId).await.unwrap();

            println!(
                "Vendor ID for slave {}: {:#04x} ({})",
                slave_idx,
                vendor_id,
                ethercrab::vendors::vendor_name(vendor_id).unwrap_or("unknown vendor")
            );

            let supported_mailbox_protocols = slave
                .read_eeprom_raw(SiiCoding::MailboxProtocol)
                .await
                .unwrap();

            let supported_mailbox_protocols =
                MailboxProtocols::from_bits(supported_mailbox_protocols as u16).unwrap();

            println!(
                "Supported mailbox protocols: {:?}",
                supported_mailbox_protocols
            );
        }
    })));

    Ok(())
}
