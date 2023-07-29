#![no_std]
#![no_main]
#![feature(type_alias_impl_trait)]

use core::{future::poll_fn, task::Poll};
use defmt_rtt as _;
use embassy_executor::Spawner;
use embassy_net::{
    driver::{Driver, RxToken, TxToken},
    Stack, StackResources,
};
use embassy_stm32::{
    bind_interrupts,
    eth::{self, generic_smi::GenericSMI, Ethernet, PacketQueue},
    peripherals::ETH,
    rng::Rng,
    time::mhz,
    Config,
};
use ethercrab::PduStorage;
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

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let mut config = Config::default();
    config.rcc.pll48 = true;
    config.rcc.sys_ck = Some(mhz(48));

    let p = embassy_stm32::init(config);

    defmt::info!("Hello World!");

    let mac_addr = [0x00, 0x00, 0xDE, 0xAD, 0xBE, 0xEF];

    let mut device = Ethernet::new(
        make_static!(PacketQueue::<8, 8>::new()),
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

    loop {
        poll_fn(|cx| {
            defmt::info!("Poll");

            let Some((rx, tx)) = device.receive(cx) else {
                defmt::info!("--> No frames");
                return Poll::Pending;
            };

            rx.consume(|rx| {
                defmt::info!("--> Rx");
            });

            tx.consume(0, |tx| {
                defmt::info!("--> Tx");
            });

            // Poll::Ready((rx, tx))
            Poll::<()>::Pending
        })
        .await;

        defmt::info!("Loop");
    }
}
