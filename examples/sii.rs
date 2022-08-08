//! Set slave addresses using `client.init()` and request pre-operational state for both slaves.
//!
//! This is designed for use with the EK1100 + EL2004.

use async_ctrlc::CtrlC;
use ethercrab::al_status::AlState;
use ethercrab::client::Client;
use ethercrab::error::PduError;
use ethercrab::register::RegisterAddress;
use ethercrab::sii::{CategoryType, SiiGeneral};
use ethercrab::slave::SlaveRef;
use ethercrab::std::tx_rx_task;
use ethercrab::timer_factory::TimerFactory;
use futures_lite::FutureExt;
use nom::multi::length_data;
use nom::number::complete::le_u8;
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

        client
            .request_slave_state(AlState::PreOp)
            .await
            .expect("Pre-op");

        // for slave_idx in 0..num_slaves {
        //     let slave = client.slave_by_index(slave_idx).expect("Slave");

        //     let vendor_id = slave.read_eeprom_raw(SiiCoding::VendorId).await.unwrap() as u32;

        //     println!(
        //         "Vendor ID for slave {}: {:#04x} ({})",
        //         slave_idx,
        //         vendor_id,
        //         ethercrab::vendors::vendor_name(vendor_id).unwrap_or("unknown vendor")
        //     );

        //     let supported_mailbox_protocols = slave
        //         .read_eeprom_raw(SiiCoding::MailboxProtocol)
        //         .await
        //         .unwrap();

        //     let supported_mailbox_protocols =
        //         MailboxProtocols::from_bits(supported_mailbox_protocols as u16).unwrap();

        //     println!(
        //         "Supported mailbox protocols: {:?}",
        //         supported_mailbox_protocols
        //     );
        // }

        let el2004 = client.slave_by_index(1).unwrap();

        let mut start = 0x0040u16;

        let mut chunk_buf = [0u8; 8];

        loop {
            let category_type = el2004
                .read_eeprom_raw(start, &mut chunk_buf)
                .await
                .map(|chunk| {
                    let category = u16::from_le_bytes(chunk[0..2].try_into().unwrap());

                    CategoryType::try_from(category).unwrap_or(CategoryType::Nop)
                })
                .unwrap();

            start += 1;

            let data_len = el2004
                .read_eeprom_raw(start, &mut chunk_buf)
                .await
                .map(|chunk| u16::from_le_bytes(chunk[0..2].try_into().unwrap()))
                .unwrap();

            start += 1;

            log::debug!(
                "Found category {:?}, data starts at {start:#06x?}, length {:#04x?} ({}) bytes",
                category_type,
                data_len,
                data_len
            );

            match category_type {
                CategoryType::Strings => parse_strings(&el2004, start).await,
                CategoryType::General => parse_general(&el2004, start, data_len).await,
                CategoryType::End => {
                    break;
                }
                _ => (),
            }

            // Next category starts after this one's data
            start += data_len;
        }
    })));

    Ok(())
}

async fn parse_strings<
    'a,
    const MAX_FRAMES: usize,
    const MAX_PDU_DATA: usize,
    const MAX_SLAVES: usize,
    TIMEOUT,
>(
    el2004: &SlaveRef<'a, MAX_FRAMES, MAX_PDU_DATA, MAX_SLAVES, TIMEOUT>,
    mut start: u16,
) where
    TIMEOUT: TimerFactory,
{
    let mut chunk_buf = [0u8; 8];

    let sl = el2004.read_eeprom_raw(start, &mut chunk_buf).await.unwrap();
    // TODO: Why does 4 work but chunk length doesn't?
    // offset += chunk.len() as u16;
    start += 4;
    log::debug!("Read {start:#06x?} {:02x?}", sl);

    // The first byte of the strings section is the number of strings contained within it
    let (num_strings, buf) = sl.split_first().expect("Split first");
    let num_strings = *num_strings;

    log::info!("Found {num_strings} strings");

    // Initialise the buffer with the remaining first read
    let mut buf = heapless::Vec::<u8, 255>::from_slice(buf).unwrap();

    for _ in 0..num_strings {
        // TODO: This loop needs splitting into a function which fills up a slice and returns it
        loop {
            let sl = el2004.read_eeprom_raw(start, &mut chunk_buf).await.unwrap();
            // TODO: Why does 4 work but chunk length doesn't?
            // offset += chunk.len() as u16;
            start += 4;
            log::debug!("Read {start:#06x?} {:02x?}", sl);
            buf.extend_from_slice(sl).expect("Buffer is full");

            let i = buf.as_slice();

            let i = match length_data::<_, _, (), _>(le_u8)(i) {
                Ok((i, string_data)) => {
                    log::info!("{:?}", String::from_utf8_lossy(string_data));

                    i
                }
                Err(e) => match e {
                    nom::Err::Incomplete(_needed) => {
                        continue;
                    }
                    nom::Err::Error(e) => panic!("Error {e:?}"),
                    nom::Err::Failure(e) => panic!("Fail {e:?}"),
                },
            };

            buf = heapless::Vec::from_slice(i).unwrap();

            break;
        }
    }
}

async fn parse_general<
    'a,
    const MAX_FRAMES: usize,
    const MAX_PDU_DATA: usize,
    const MAX_SLAVES: usize,
    TIMEOUT,
>(
    el2004: &SlaveRef<'a, MAX_FRAMES, MAX_PDU_DATA, MAX_SLAVES, TIMEOUT>,
    mut start: u16,
    len: u16,
) where
    TIMEOUT: TimerFactory,
{
    let mut chunk_buf = [0u8; 8];

    let len = usize::from(len);

    // Chunks are read in multiples of 4 or 8 and we need at least 18 bytes
    let mut buf = heapless::Vec::<u8, 24>::new();

    // TODO: This loop needs splitting into a function which fills up a slice and returns it
    let buf = loop {
        let sl = el2004.read_eeprom_raw(start, &mut chunk_buf).await.unwrap();
        log::debug!("Read {start:#06x?} {:02x?}", sl);
        // TODO: Why does 4 work but chunk length doesn't?
        // offset += chunk.len() as u16;
        start += 4;

        buf.extend_from_slice(sl).expect("Buffer is full");

        if buf.len() >= len {
            break &buf[0..len];
        }
    };

    let (_, general) = SiiGeneral::parse(buf).expect("General parse");

    log::info!("General\n{:#?}", general);
}
