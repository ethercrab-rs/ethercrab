#![no_std]
#![no_main]

use core::{future::poll_fn, task::Poll};
use defmt_rtt as _;
use embassy_executor::Spawner;
use embassy_net::driver::{Driver, LinkState, RxToken, TxToken};
use embassy_stm32::{
    bind_interrupts,
    eth::{self, generic_smi::GenericSMI, Ethernet, PacketQueue},
    gpio::{Level, Output, Speed},
    peripherals::{ETH, PB7},
    time::Hertz,
    Config,
};
use embassy_time::{Duration, Instant, Timer};
use ethercrab::{MainDevice, MainDeviceConfig, PduRx, PduStorage, PduTx, SendableFrame, Timeouts};
use panic_probe as _;
use static_cell::StaticCell;

bind_interrupts!(struct Irqs {
    ETH => eth::InterruptHandler;
});

/// Maximum number of SubDevices that can be stored. This must be a power of 2 greater than 1.
const MAX_SUBDEVICES: usize = 16;
/// Maximum PDU data payload size - set this to the max PDI size or higher.
const MAX_PDU_DATA: usize = PduStorage::element_size(256);
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
                .send_blocking(|ethernet_frame| {
                    tx_buf[0..ethernet_frame.len()].copy_from_slice(ethernet_frame);

                    Ok(ethernet_frame.len())
                })
                .map_err(|e| defmt::error!("Send blocking: {}", e)));
        });
    }

    poll_fn(|ctx| {
        pdu_tx.replace_waker(ctx.waker());

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

        Poll::Pending
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

    {
        use embassy_stm32::rcc::*;
        config.rcc.hse = Some(Hse {
            freq: Hertz(8_000_000),
            mode: HseMode::Bypass,
        });
        config.rcc.pll_src = PllSource::HSE;
        config.rcc.pll = Some(Pll {
            prediv: PllPreDiv::DIV4,
            mul: PllMul::MUL180,
            divp: Some(PllPDiv::DIV2), // 8mhz / 4 * 180 / 2 = 180Mhz.
            divq: None,
            divr: None,
        });
        config.rcc.ahb_pre = AHBPrescaler::DIV1;
        config.rcc.apb1_pre = APBPrescaler::DIV4;
        config.rcc.apb2_pre = APBPrescaler::DIV2;
        config.rcc.sys = Sysclk::PLL1_P;
    }

    let p = embassy_stm32::init(config);

    defmt::info!("Hello World!");

    let mac_addr = [0x00, 0x00, 0xDE, 0xAD, 0xBE, 0xEF];

    let led = Output::new(p.PB7, Level::High, Speed::Low);
    defmt::unwrap!(spawner.spawn(blinky(led)));

    let (tx, rx, pdu_loop) = defmt::unwrap!(PDU_STORAGE.try_split());

    static PACKETS: StaticCell<PacketQueue<8, 8>> = StaticCell::new();
    let device = {
        let mut device = Ethernet::new(
            PACKETS.init(PacketQueue::<8, 8>::new()),
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
            GenericSMI::new(0),
            mac_addr,
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

    let maindevice = MainDevice::new(pdu_loop, Timeouts::default(), MainDeviceConfig::default());

    defmt::info!("Begin loop");

    let group = defmt::unwrap!(
        maindevice
            .init_single_group::<MAX_SUBDEVICES, PDI_LEN>(|| Instant::now().as_micros() * 1000)
            .await
    );

    defmt::info!("Discovered {} SubDevices", group.len());

    let mut group = defmt::unwrap!(group.into_op(&maindevice).await);

    for subdevice in group.iter(&maindevice) {
        let (i, o) = subdevice.io_raw();

        defmt::info!(
            "-> SubDevice {:#06x} {} inputs: {} bytes, outputs: {} bytes",
            subdevice.configured_address(),
            subdevice.name(),
            i.len(),
            o.len()
        );
    }

    loop {
        defmt::unwrap!(group.tx_rx(&maindevice).await);

        // Increment every output byte for every SubDevice by one
        for mut subdevice in group.iter(&maindevice) {
            let (_i, o) = subdevice.io_raw_mut();

            for byte in o.iter_mut() {
                *byte = byte.wrapping_add(1);
            }
        }

        Timer::after(embassy_time::Duration::from_millis(5)).await;
    }
}
