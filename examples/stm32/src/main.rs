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
use panic_probe as _;
use static_cell::make_static;

bind_interrupts!(struct Irqs {
    ETH => eth::InterruptHandler;
});

type Device = Ethernet<'static, ETH, GenericSMI>;

// #[embassy_executor::task]
// async fn net_task(stack: &'static Stack<Device>) -> ! {
//     stack.run().await
// }

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let mut config = Config::default();
    config.rcc.pll48 = true;
    config.rcc.sys_ck = Some(mhz(48));

    let p = embassy_stm32::init(config);

    defmt::info!("Hello World!");

    // // Generate random seed.
    // let mut rng = Rng::new(p.RNG);
    // let mut seed = [0; 8];
    // rng.fill_bytes(&mut seed);
    // let seed = u64::from_le_bytes(seed);

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

    loop {
        poll_fn(|cx| {
            defmt::info!("Poll");

            let Some((rx, tx)) = device.receive(cx) else {
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
