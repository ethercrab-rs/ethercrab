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
const MAX_PDU_DATA: usize = 1100;
/// Maximum number of EtherCAT frames that can be in flight at any one time.
const MAX_FRAMES: usize = 16;
/// Maximum total PDI length.
const PDI_LEN: usize = 64;

static PDU_STORAGE: PduStorage<MAX_FRAMES, MAX_PDU_DATA> = PduStorage::new();

#[embassy_executor::task(pool_size = 4)]
async fn tx_rx_task(
    mut device: Ethernet<'static, ETH, GenericSMI>,
    mut pdu_tx: PduTx<'static>,
    mut pdu_rx: PduRx<'static>,
) -> ! {
    defmt::info!("Spawn TX/RX");

    fn send_ecat(tx: embassy_stm32::eth::TxToken<'_, '_>, frame: SendableFrame<'_>) {
        // let len = frame.len();

        tx.consume(frame.len(), |tx_buf| {
            let _ = frame.send_blocking(tx_buf, |_ethernet_frame| {
                // Frame is copied into `tx_buf` inside `send_blocking` so we don't need to do
                // anything here. The frame is sent once the outer closure in `tx.consume` ends.

                Ok(())
            });
        });
    }

    poll_fn(|ctx| {
        // defmt::info!("Poll");

        pdu_tx.lock_waker().replace(ctx.waker().clone());

        if let Some((rx, tx)) = device.receive(ctx) {
            // defmt::info!("--> Rx and tx available");

            rx.consume(|frame| {
                // let mut f = smoltcp::wire::EthernetFrame::new_unchecked(frame);

                // if f.ethertype() == smoltcp::wire::EthernetProtocol::Unknown(0x88a4) {
                //     defmt::info!("--> ECAT RESPONSE!");

                //     defmt::info!(
                //         "type {:?}, dst {:?} src {:?}",
                //         f.ethertype(),
                //         f.dst_addr(),
                //         f.src_addr()
                //     );
                // }

                defmt::unwrap!(pdu_rx.receive_frame(frame).map_err(|_| {
                    defmt::error!("RX");
                }));
            });

            if let Some(ethercat_frame) = pdu_tx.next_sendable_frame() {
                defmt::info!("Sennnddddd FROM RX");

                send_ecat(tx, ethercat_frame);
            }

            // Wake again to continue processing any queued packets
            ctx.waker().wake_by_ref();
        } else if let Some(tx) = device.transmit(ctx) {
            // defmt::info!("--> Tx available");

            if let Some(ethercat_frame) = pdu_tx.next_sendable_frame() {
                defmt::info!("Sennnddddd");

                send_ecat(tx, ethercat_frame);

                // Wake again to continue processing any queued packets
                ctx.waker().wake_by_ref();
            }
        } else {
            // defmt::info!("--> No stuff");
        }

        Poll::<!>::Pending
    })
    .await
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let mut config = Config::default();
    config.rcc.pll48 = true;
    config.rcc.sys_ck = Some(mhz(48));

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

    let (tx, rx, pdu_loop) = PDU_STORAGE.try_split().expect("can only split once");

    let client = Client::new(
        pdu_loop,
        Timeouts {
            // pdu: core::time::Duration::from_secs(5),
            ..Timeouts::default()
        },
        ClientConfig::default(),
    );

    defmt::unwrap!(spawner.spawn(tx_rx_task(device, tx, rx)));

    // // Do nothing here for now except let the tx/rx task run
    // core::future::pending::<()>().await;

    //

    defmt::info!("Loop");

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

        Timer::after(embassy_time::Duration::from_secs(1)).await;
    }
}
