#![no_std]
#![no_main]
#![feature(type_alias_impl_trait)]
#![feature(never_type)]

use core::{future::poll_fn, task::Poll};
use defmt_rtt as _;
use embassy_executor::Spawner;
use embassy_net::driver::{Driver, LinkState, RxToken, TxToken};
use embassy_stm32::{
    bind_interrupts,
    eth::{self, generic_smi::GenericSMI, Ethernet, PacketQueue},
    gpio::{Level, Output, Speed},
    peripherals::{ETH, PB7},
    time::mhz,
    Config,
};
use embassy_time::{Duration, Instant, Timer};
use ethercrab::{Client, ClientConfig, PduRx, PduStorage, PduTx, SendableFrame, Timeouts};
use panic_probe as _;
use static_cell::make_static;

bind_interrupts!(struct Irqs {
    ETH => eth::InterruptHandler;
});

/// Maximum number of slaves that can be stored. This must be a power of 2 greater than 1.
const MAX_SLAVES: usize = 16;
/// Maximum PDU data payload size - set this to the max PDI size or higher.
const MAX_PDU_DATA: usize = 256;
/// Maximum number of EtherCAT frames that can be in flight at any one time.
const MAX_FRAMES: usize = 8;
/// Maximum total PDI length.
const PDI_LEN: usize = 256;

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
            defmt::unwrap!(frame
                .send_blocking(tx_buf, |ethernet_frame| {
                    // Frame is copied into `tx_buf` inside `send_blocking` so we don't need to do
                    // anything here. The frame is sent once the outer closure in `tx.consume` ends.

                    Ok(ethernet_frame.len())
                })
                .map_err(|e| defmt::error!("Send blocking: {}", e)));
        });
    }

    poll_fn(|ctx| {
        pdu_tx.waker().replace(ctx.waker().clone());

        if let Some((rx, tx)) = device.receive(ctx) {
            rx.consume(|frame| {
                defmt::unwrap!(pdu_rx.receive_frame(frame));
            });

            if let Some(ethercat_frame) = pdu_tx.next_sendable_frame() {
                send_ecat(tx, ethercat_frame);

                // Wake the poll_fn again so that any still-queued frames can be sent.
                ctx.waker().wake_by_ref();
            }
        } else if let Some(tx) = device.transmit(ctx) {
            if let Some(ethercat_frame) = pdu_tx.next_sendable_frame() {
                send_ecat(tx, ethercat_frame);

                // Wake the poll_fn again so that any still-queued frames can be sent.
                ctx.waker().wake_by_ref();
            }
        }

        Poll::<!>::Pending
    })
    .await
}

#[embassy_executor::task]
async fn blinky(mut led: Output<'static, PB7>) -> ! {
    loop {
        led.set_high();
        Timer::after(Duration::from_millis(250)).await;

        led.set_low();
        Timer::after(Duration::from_millis(250)).await;
    }
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let mut config = Config::default();
    config.rcc.pll48 = true;
    config.rcc.sys_ck = Some(mhz(96));

    let p = embassy_stm32::init(config);

    defmt::info!("Hello World!");

    let mac_addr = [0x00, 0x00, 0xDE, 0xAD, 0xBE, 0xEF];

    let led = Output::new(p.PB7, Level::High, Speed::Low);
    defmt::unwrap!(spawner.spawn(blinky(led)));

    let (tx, rx, pdu_loop) = defmt::unwrap!(PDU_STORAGE.try_split());

    let device = {
        let mut device = Ethernet::new(
            make_static!(PacketQueue::<4, 4>::new()),
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

        defmt::info!("Waiting for Ethernet link up...");

        let now = Instant::now();

        let up_after = poll_fn(|ctx| match device.link_state(ctx) {
            LinkState::Down => Poll::Pending,
            LinkState::Up => Poll::Ready(now.elapsed()),
        })
        .await;

        defmt::info!("Link is up after {} ms", up_after.as_millis());

        device
    };

    defmt::unwrap!(spawner.spawn(tx_rx_task(device, tx, rx)));

    let client = Client::new(pdu_loop, Timeouts::default(), ClientConfig::default());

    defmt::info!("Begin loop");

    let group = defmt::unwrap!(client.init_single_group::<MAX_SLAVES, PDI_LEN>().await);

    defmt::info!("Discovered {} slaves", group.len());

    let mut group = defmt::unwrap!(group.into_op(&client).await);

    for slave in group.iter(&client) {
        let (i, o) = slave.io_raw();

        defmt::info!(
            "-> Slave {:#06x} {} inputs: {} bytes, outputs: {} bytes",
            slave.configured_address(),
            slave.name(),
            i.len(),
            o.len()
        );
    }

    loop {
        defmt::unwrap!(group.tx_rx(&client).await);

        // Increment every output byte for every slave device by one
        for slave in group.iter(&client) {
            let (_i, o) = slave.io_raw();

            for byte in o.iter_mut() {
                *byte = byte.wrapping_add(1);
            }
        }

        Timer::after(embassy_time::Duration::from_millis(5)).await;
    }
}
