//! A refactor of brd-waker to contain TX/RX in a single task, only passing data/wakers.

use async_ctrlc::CtrlC;
use ethercrab::client::Client;
use ethercrab::register::RegisterAddress;
use futures_lite::FutureExt;
use smol::LocalExecutor;

#[cfg(target_os = "windows")]
// ASRock NIC
// const INTERFACE: &str = "TODO";
// USB NIC
const INTERFACE: &str = "\\Device\\NPF_{DCEDC919-0A20-47A2-9788-FC57D0169EDB}";
#[cfg(not(target_os = "windows"))]
const INTERFACE: &str = "eth0";

fn main() {
    let local_ex = LocalExecutor::new();

    let ctrlc = CtrlC::new().expect("cannot create Ctrl+C handler?");

    futures_lite::future::block_on(local_ex.run(ctrlc.race(async {
        let client = Client::<16, 16, smol::Timer>::new();

        local_ex
            .spawn(client.tx_rx_task(INTERFACE).unwrap())
            .detach();

        let res = client.brd::<[u8; 1]>(RegisterAddress::Type).await.unwrap();
        println!("RESULT: {:#02x?}", res);
        let res = client.brd::<u16>(RegisterAddress::Build).await.unwrap();
        println!("RESULT: {:#04x?}", res);
    })));
}
