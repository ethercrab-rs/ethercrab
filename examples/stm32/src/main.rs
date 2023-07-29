#![no_std]
#![no_main]
#![feature(type_alias_impl_trait)]
#![feature(never_type)]

use core::{future::poll_fn, task::Poll};
use defmt_rtt as _;
use embassy_executor::Spawner;
use embassy_net::driver::{Driver, RxToken, TxToken};
use embassy_stm32::{
    bind_interrupts,
    eth::{self, generic_smi::GenericSMI, Ethernet, PacketQueue},
    peripherals::ETH,
    time::mhz,
    Config,
};
use embassy_time::Timer;
use ethercrab::{AlStatusCode, Client, ClientConfig, PduRx, PduStorage, PduTx, Timeouts};
use panic_probe as _;
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

#[embassy_executor::task]
async fn tx_rx_task(
    mut device: Ethernet<'static, ETH, GenericSMI>,
    mut tx: PduTx<'static>,
    mut rx: PduRx<'static>,
) -> ! {
    defmt::info!("Spawn TX/RX");

    poll_fn(|ctx| {
        defmt::info!("Poll");

        let mut tx_waker = tx.lock_waker();

        if let Some(frame) = tx.next_sendable_frame() {
            defmt::info!("--> Found sendable frame {} bytes", frame.len());

            let tx_token = device.transmit(ctx).expect("no txitches?");

            tx_token.consume(frame.len(), |buf| {
                frame.write_ethernet_packet(buf).expect("write frame");
            });
        }

        if let Some((rx, tx)) = device.receive(ctx) {
            rx.consume(|rx| {
                let frame = smoltcp::wire::EthernetFrame::new_unchecked(rx);

                match frame.ethertype() {
                    smoltcp::wire::EthernetProtocol::Unknown(value) if value == 0x88a4 => {
                        defmt::info!("--> Rx ethercat")
                    }
                    other => defmt::info!("--> Rx non-ethercat {:?}", other),
                }
            });

            // tx.consume(0, |tx| {
            //     defmt::info!("--> Tx");
            // });
        } else {
            defmt::info!("--> No frames")
        }

        tx_waker.replace(ctx.waker().clone());

        // Poll::Ready((rx, tx))
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

    let client = Client::new(pdu_loop, Timeouts::default(), ClientConfig::default());

    defmt::unwrap!(spawner.spawn(tx_rx_task(device, tx, rx)));

    // // Do nothing here for now except let the tx/rx task run
    // core::future::pending::<()>().await;

    loop {
        //

        defmt::info!("Loop");

        if let Ok((status, wkc)) = client
            .brd::<AlStatusCode>(ethercrab::RegisterAddress::AlStatus)
            .await
        {
            defmt::info!("--> WKC {}", wkc);
        }

        Timer::after(embassy_time::Duration::from_secs(1)).await;
    }
}
