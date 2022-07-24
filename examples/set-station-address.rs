//! A refactor of brd-waker to contain TX/RX in a single task, only passing data/wakers.

use std::time::Duration;

use async_ctrlc::CtrlC;
use ethercrab::client::Client;
use ethercrab::pdu::PduError;
use ethercrab::register::RegisterAddress;
use futures_lite::FutureExt;
use smol::LocalExecutor;

#[cfg(target_os = "windows")]
// ASRock NIC
// const INTERFACE: &str = "TODO";
// USB NIC
// const INTERFACE: &str = "\\Device\\NPF_{DCEDC919-0A20-47A2-9788-FC57D0169EDB}";
// Silver USB NIC
const INTERFACE: &str = "\\Device\\NPF_{CC0908D5-3CB8-46D6-B8A2-575D0578008D}";
#[cfg(not(target_os = "windows"))]
const INTERFACE: &str = "eth0";

fn main() -> Result<(), PduError> {
    let local_ex = LocalExecutor::new();

    let ctrlc = CtrlC::new().expect("cannot create Ctrl+C handler?");

    futures_lite::future::block_on(local_ex.run(ctrlc.race(async {
        let client = Client::<16, 16, smol::Timer>::new();

        local_ex
            .spawn(client.tx_rx_task(INTERFACE).unwrap())
            .detach();

        let (_res, num_slaves) = client.brd::<u8>(RegisterAddress::Type).await.unwrap();

        println!("Discovered {num_slaves} slaves");

        let client2 = client.clone();

        local_ex
            .spawn(async move {
                loop {
                    for slave_idx in 0..num_slaves {
                        let (configured_address, working_counter) = client2
                            .aprd::<u16>(slave_idx, RegisterAddress::AlStatus)
                            .await
                            .unwrap();

                        assert_eq!(
                            working_counter, 1,
                            "Failed to read AL status for slave {slave_idx}"
                        );

                        println!("Slave {slave_idx} AL status: {configured_address}");

                        async_io::Timer::after(Duration::from_millis(500)).await;
                    }
                }
            })
            .detach();

        for slave_idx in 0..num_slaves {
            println!("Read slave {slave_idx}");

            let (configured_station_address, working_counter) = client
                .aprd::<u16>(slave_idx, RegisterAddress::ConfiguredStationAddress)
                .await
                .unwrap();

            assert_eq!(
                working_counter, 1,
                "Failed to read station address for slave index {slave_idx}"
            );

            println!(
                "Slave {slave_idx} configured station address: {configured_station_address:?}"
            );
        }

        // Write configured address with 0x1000 base offset
        for slave_idx in 0..num_slaves {
            let address = 0x1000 + slave_idx;

            println!("Setting slave {slave_idx} address {address:#04x}");

            let (_, working_counter) = client
                .apwr(
                    slave_idx,
                    RegisterAddress::ConfiguredStationAddress,
                    address,
                )
                .await
                .unwrap();

            assert_eq!(
                working_counter, 1,
                "Slave idx {slave_idx} failed to set address {address}"
            );
        }

        'outer: loop {
            // Read configured address using `APRD`
            for slave_idx in 0..num_slaves {
                let (configured_address, working_counter) = client
                    .aprd::<u16>(slave_idx, RegisterAddress::ConfiguredStationAddress)
                    .await
                    .unwrap();

                assert_eq!(
                    working_counter, 1,
                    "Failed to read configured address for slave {slave_idx}"
                );

                println!("Slave {slave_idx} configured address: {configured_address}");

                if configured_address != slave_idx {
                    break 'outer;
                }
            }

            async_io::Timer::after(Duration::from_millis(1000)).await;
        }

        // Read configured address using `FPRD`
        for slave_idx in 0..num_slaves {
            let address = 0x1000 + slave_idx;

            let (configured_address, working_counter) = client
                .fprd::<u16>(address, RegisterAddress::ConfiguredStationAddress)
                .await
                .unwrap();

            assert_eq!(
                working_counter, 1,
                "Failed to read configured address for slave {slave_idx}"
            );

            println!("Slave {slave_idx} configured address: {configured_address}");
        }

        async_io::Timer::after(Duration::from_millis(5000)).await;
    })));

    Ok(())
}
