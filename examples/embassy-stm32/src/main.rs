#![no_std]
#![no_main]
#![feature(type_alias_impl_trait)]
#![feature(never_type)]

use core::{future::poll_fn, task::Poll};
use defmt_rtt as _;
use embassy_executor::Spawner;
use embassy_net::{
    driver::{Driver, RxToken, TxToken},
    EthernetAddress,
};
use embassy_stm32::{
    bind_interrupts,
    eth::{self, generic_smi::GenericSMI, Ethernet, PacketQueue},
    peripherals::ETH,
    time::mhz,
    Config,
};
use embassy_time::Timer;
use embedded_io::asynch::{Read, Write};
use ethercrab::{
    AlStatusCode, Client, ClientConfig, PduRx, PduStorage, PduTx, SendableFrame, Timeouts,
};
use panic_probe as _;
use smoltcp::wire::EthernetProtocol;
use static_cell::make_static;

bind_interrupts!(struct Irqs {
    ETH => eth::InterruptHandler;
});

type Device = Ethernet<'static, ETH, GenericSMI>;

/// Maximum number of slaves that can be stored. This must be a power of 2 greater than 1.
const MAX_SLAVES: usize = 16;
/// Maximum PDU data payload size - set this to the max PDI size or higher.
const MAX_PDU_DATA: usize = 64;
/// Maximum number of EtherCAT frames that can be in flight at any one time.
const MAX_FRAMES: usize = 8;
/// Maximum total PDI length.
const PDI_LEN: usize = 64;

static PDU_STORAGE: PduStorage<MAX_FRAMES, MAX_PDU_DATA> = PduStorage::new();

#[embassy_executor::task]
async fn tx_rx_task(
    mut device: Ethernet<'static, ETH, GenericSMI>,
    mut pdu_tx: PduTx<'static>,
    mut pdu_rx: PduRx<'static>,
) -> ! {
    defmt::info!("Spawn TX/RX");

    #[inline(always)]
    fn send_ecat(tx: embassy_stm32::eth::TxToken<'_, '_>, frame: SendableFrame<'_>) {
        tx.consume(frame.len(), |tx_buf| {
            let _ = frame
                .send_blocking(tx_buf, |_ethernet_frame| {
                    // Frame is copied into `tx_buf` inside `send_blocking` so we don't need to do
                    // anything here. The frame is sent once the outer closure in `tx.consume` ends.

                    Ok(())
                })
                .map_err(|e| defmt::error!("Send blocking: {}", e));
        });
    }

    poll_fn(|ctx| {
        defmt::info!("Poll");

        pdu_tx.lock_waker().replace(ctx.waker().clone());

        loop {
            if let Some((rx, tx)) = device.receive(ctx) {
                defmt::info!("TX WITH RX");

                rx.consume(|frame| {
                    defmt::unwrap!(pdu_rx.receive_frame(frame));
                });

                if let Some(ethercat_frame) = pdu_tx.next_sendable_frame() {
                    defmt::info!("Sennnddddd FROM RX");

                    send_ecat(tx, ethercat_frame);
                }
            } else if let Some(tx) = device.transmit(ctx) {
                defmt::info!("TX ONLY");

                if let Some(ethercat_frame) = pdu_tx.next_sendable_frame() {
                    defmt::info!("Sennnddddd");

                    send_ecat(tx, ethercat_frame);
                } else {
                    break;
                }
            } else {
                break;
            }
        }

        Poll::<!>::Pending
    })
    .await
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let mut config = Config::default();
    // config.rcc.pll48 = true;
    config.rcc.sys_ck = Some(mhz(96));

    let p = embassy_stm32::init(config);

    defmt::info!("Hello World!");

    let mac_addr = [0x00, 0x00, 0xDE, 0xAD, 0xBE, 0xEF];

    let device = Ethernet::new(
        make_static!(PacketQueue::<2, 2>::new()),
        p.ETH,
        Irqs,
        p.PA1,
        p.PA2,
        p.PC1,
        p.PA7,
        p.PC4,
        p.PC5,
        p.PG13,
        p.PB13,
        p.PG11,
        GenericSMI::new(),
        mac_addr,
        0,
    );

    let (tx, rx, pdu_loop) = defmt::unwrap!(PDU_STORAGE.try_split());

    defmt::unwrap!(spawner.spawn(tx_rx_task(device, tx, rx)));

    let client = Client::new(
        pdu_loop,
        Timeouts {
            // pdu: core::time::Duration::from_secs(5),
            ..Timeouts::default()
        },
        ClientConfig::default(),
    );

    defmt::info!("Begin loop");

    let mut group = defmt::unwrap!(client.init_single_group::<MAX_SLAVES, PDI_LEN>().await);

    // defmt::info!("Discovered {} slaves", group.len());

    // let mut group = defmt::unwrap!(group.into_op(&client).await);

    // for slave in group.iter(&client) {
    //     let (i, o) = slave.io_raw();

    //     defmt::info!(
    //         "-> Slave {:#06x} {} inputs: {} bytes, outputs: {} bytes",
    //         slave.configured_address(),
    //         slave.name(),
    //         i.len(),
    //         o.len()
    //     );
    // }

    loop {
        match client
            .brd::<u16>(ethercrab::RegisterAddress::AlStatus)
            .await
        {
            Ok((status, wkc)) => {
                defmt::info!("--> WKC {}", wkc);
            }
            Err(e) => {
                defmt::error!("--> BRD fail: {}", e);
            }
        }

        // defmt::unwrap!(group.tx_rx(&client).await);

        // // Increment every output byte for every slave device by one
        // for slave in group.iter(&client) {
        //     let (_i, o) = slave.io_raw();

        //     for byte in o.iter_mut() {
        //         *byte = byte.wrapping_add(1);
        //     }
        // }

        Timer::after(embassy_time::Duration::from_secs(1)).await;
    }
}
