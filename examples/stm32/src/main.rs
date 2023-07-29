#![no_std]
#![no_main]
#![feature(type_alias_impl_trait)]

use embassy_executor::Spawner;
use embassy_net::{Stack, StackResources};
use embassy_stm32::rng::Rng;
use embassy_stm32::time::mhz;
use embassy_stm32::usb_otg::Driver;
use embassy_stm32::{bind_interrupts, peripherals, usb_otg, Config};
use embedded_io::asynch::Write;
use static_cell::make_static;
use {defmt_rtt as _, panic_probe as _};

const MTU: usize = 1514;

// #[embassy_executor::task]
// async fn net_task(stack: &'static Stack<Device<'static, MTU>>) -> ! {
//     stack.run().await
// }

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    defmt::info!("Hello World!");

    loop {}
}
