//! EEPROM dump tool. Currently looks for second found slave and dumps it's EEPROM to a file.

use async_ctrlc::CtrlC;
use ethercrab::al_status::AlState;
use ethercrab::client::Client;
use ethercrab::error::PduError;
use ethercrab::register::RegisterAddress;
use ethercrab::sii::SiiCoding;
use ethercrab::std::tx_rx_task;
use futures_lite::FutureExt;
use smol::LocalExecutor;
use std::fs::File;
use std::io::Write;
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

        client
            .request_slave_state(AlState::PreOp)
            .await
            .expect("Pre-op");

        let el2004 = client.slave_by_index(1).unwrap();

        let mut start = 0x0040u16;

        let mut chunk_buf = [0u8; 8];

        let mut f = File::create("./eeprom-dump.bin").unwrap();
        let mut read_count = 0u16;

        let size = el2004
            .read_eeprom_raw(SiiCoding::Size, &mut chunk_buf)
            .await
            .map(|chunk| {
                let size_kibibits = u16::from_le_bytes(chunk[0..2].try_into().unwrap());

                // Yes, +1 as per ETG1000.6 5.4 SII coding
                (size_kibibits + 1) / 8
            })
            .unwrap();

        let size = size * 1024;

        log::info!("Found EEPROM size {} bytes", size);

        loop {
            let chunk = el2004.read_eeprom_raw(start, &mut chunk_buf).await.unwrap();

            // TODO: Why does 4 work but chunk length doesn't?
            // start += chunk.len() as u16;
            start += 4;

            read_count += chunk.len() as u16;

            f.write_all(chunk).unwrap();

            if read_count >= size {
                break;
            }
        }

        log::info!("Dumped {} bytes", read_count);
    })));

    Ok(())
}
