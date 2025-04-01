#![no_std]
#![no_main]

use defmt_rtt as _;
use panic_probe as _;

mod common;

#[rtic::app(device = stm32_eth::stm32, dispatchers = [SPI1])]
mod app {
    use crate::common::EthernetPhy;
    use core::task::Poll;
    use fugit::ExtU64;
    use ieee802_3_miim::{phy::PhySpeed, Phy};
    use stm32_eth::{
        dma::{EthernetDMA, PacketId, RxRingEntry, TxRingEntry},
        mac::Speed,
        ptp::{EthernetPTP, Timestamp},
        Parts,
    };
    use systick_monotonic::Systick;

    #[local]
    struct Local {}

    #[shared]
    struct Shared {
        dma: EthernetDMA<'static, 'static>,
        ptp: EthernetPTP,
        tx_id: Option<(u32, Timestamp)>,
        scheduled_time: Option<Timestamp>,
    }

    #[monotonic(binds = SysTick, default = true)]
    type Monotonic = Systick<1000>;

    #[init(local = [
        rx_ring: [RxRingEntry; 2] = [RxRingEntry::new(),RxRingEntry::new()],
        tx_ring: [TxRingEntry; 2] = [TxRingEntry::new(),TxRingEntry::new()],
    ])]
    fn init(cx: init::Context) -> (Shared, Local) {
        defmt::info!("Pre-init");
        let core = cx.core;
        let p = cx.device;

        let rx_ring = cx.local.rx_ring;
        let tx_ring = cx.local.tx_ring;

        let (clocks, gpio, ethernet) = crate::common::setup_peripherals(p);

        let rcc = device_peripherals.RCC;
        let rcc = rcc.constrain();
        let _clocks = rcc.cfgr.sysclk(100.MHz()).pclk1(36.MHz()).freeze();

        let systick_mono_token = rtic_monotonics::create_systick_token!();
        Systick::start(cx.core.SYST, 100_000_000, systick_mono_token);

        defmt::info!("Setting up pins");
        let (pins, mdio, mdc, pps) = crate::common::setup_pins(gpio);

        defmt::info!("Configuring ethernet");

        let Parts { dma, mac, mut ptp } =
            stm32_eth::new_with_mii(ethernet, rx_ring, tx_ring, clocks, pins, mdio, mdc).unwrap();

        ptp.enable_pps(pps);

        defmt::info!("Enabling interrupts");
        dma.enable_interrupt();

        match EthernetPhy::from_miim(mac, 0) {
            Ok(mut phy) => {
                defmt::info!(
                    "Resetting PHY as an extra step. Type: {}",
                    phy.ident_string()
                );

                phy.phy_init();

                defmt::info!("Waiting for link up.");

                while !phy.phy_link_up() {}

                defmt::info!("Link up.");

                if let Some(speed) = phy.speed().map(|s| match s {
                    PhySpeed::HalfDuplexBase10T => Speed::HalfDuplexBase10T,
                    PhySpeed::FullDuplexBase10T => Speed::FullDuplexBase10T,
                    PhySpeed::HalfDuplexBase100Tx => Speed::HalfDuplexBase100Tx,
                    PhySpeed::FullDuplexBase100Tx => Speed::FullDuplexBase100Tx,
                }) {
                    phy.get_miim().set_speed(speed);
                    defmt::info!("Detected link speed: {}", speed);
                } else {
                    defmt::warn!("Failed to detect link speed.");
                }
            }
            Err(_) => {
                defmt::info!("Not resetting unsupported PHY. Cannot detect link speed.");
            }
        };

        sender::spawn().ok();

        (
            Shared {
                dma,
                tx_id: None,
                scheduled_time: None,
                ptp,
            },
            Local {},
        )
    }

    #[task(shared = [dma, tx_id, ptp, scheduled_time], local = [tx_id_ctr: u32 = 0x8000_0000])]
    async fn sender(cx: sender::Context) {
        sender::spawn_after(1u64.secs()).ok();

        const SIZE: usize = 42;

        // Obtain the current time to use as the "TX time" of our frame. It is clearly
        // incorrect, but works well enough in low-activity systems (such as this example).
        let now = (cx.shared.ptp, cx.shared.scheduled_time).lock(|ptp, sched_time| {
            let now = EthernetPTP::get_time();
            #[cfg(not(feature = "stm32f107"))]
            {
                let in_half_sec = now
                    + Timestamp::new(
                        false,
                        0,
                        stm32_eth::ptp::Subseconds::new_from_nanos(500_000_000).unwrap(),
                    );
                ptp.configure_target_time_interrupt(in_half_sec);
            }
            *sched_time = Some(now);

            now
        });

        const DST_MAC: [u8; 6] = [0xAB, 0xCD, 0xEF, 0x12, 0x34, 0x56];
        const SRC_MAC: [u8; 6] = [0x00, 0x00, 0xDE, 0xAD, 0xBE, 0xEF];
        const ETH_TYPE: [u8; 2] = [0xFF, 0xFF]; // Custom/unknown ethertype

        let tx_id_ctr = cx.local.tx_id_ctr;
        (cx.shared.dma, cx.shared.tx_id).lock(|dma, tx_id| {
            let tx_id_val = *tx_id_ctr;
            dma.send(SIZE, Some(PacketId(tx_id_val)), |buf| {
                // Write the Ethernet Header and the current timestamp value to
                // the frame.
                buf[0..6].copy_from_slice(&DST_MAC);
                buf[6..12].copy_from_slice(&SRC_MAC);
                buf[12..14].copy_from_slice(&ETH_TYPE);
                buf[14..22].copy_from_slice(&now.raw().to_be_bytes());
            })
            .ok();
            *tx_id = Some((tx_id_val, now));
            *tx_id_ctr += 1;
            *tx_id_ctr |= 0x8000_0000;
        });
    }

    #[task(binds = ETH, shared = [dma, tx_id, ptp, scheduled_time], local = [pkt_id_ctr: u32 = 0], priority = 2)]
    fn eth_interrupt(cx: eth_interrupt::Context) {
        let packet_id_ctr = cx.local.pkt_id_ctr;

        (
            cx.shared.dma,
            cx.shared.tx_id,
            cx.shared.ptp,
            cx.shared.scheduled_time,
        )
            .lock(|dma, tx_id, ptp, _sched_time| {
                let int_reason = stm32_eth::eth_interrupt_handler();

                #[cfg(not(feature = "stm32f107"))]
                {
                    if int_reason.time_passed {
                        if let Some(sched_time) = _sched_time.take() {
                            let now = EthernetPTP::get_time();
                            defmt::info!(
                                "Got a timestamp interrupt {} seconds after scheduling",
                                now - sched_time
                            );
                        }
                    }
                }

                loop {
                    let packet_id = PacketId(*packet_id_ctr);
                    let rx_timestamp = if let Ok(packet) = dma.recv_next(Some(packet_id.clone())) {
                        let mut dst_mac = [0u8; 6];
                        dst_mac.copy_from_slice(&packet[..6]);

                        let ts = if let Some(timestamp) = packet.timestamp() {
                            timestamp
                        } else {
                            continue;
                        };

                        defmt::debug!("RX timestamp: {}", ts);

                        if dst_mac == [0xAB, 0xCD, 0xEF, 0x12, 0x34, 0x56] {
                            let mut timestamp_data = [0u8; 8];
                            timestamp_data.copy_from_slice(&packet[14..22]);
                            let raw = i64::from_be_bytes(timestamp_data);

                            let timestamp = Timestamp::new_raw(raw);

                            defmt::debug!("Contained TX timestamp: {}", ts);

                            let diff = timestamp - ts;

                            defmt::info!("Difference between TX and RX time: {}", diff);

                            let addend = ptp.addend();
                            let nanos = diff.nanos() as u64;

                            if nanos <= 20_000 {
                                let p1 = ((nanos * addend as u64) / 1_000_000_000) as u32;

                                defmt::debug!("Addend correction value: {}", p1);

                                if diff.is_negative() {
                                    ptp.set_addend(addend - p1 / 2);
                                } else {
                                    ptp.set_addend(addend + p1 / 2);
                                };
                            } else {
                                defmt::warn!("Updated time.");
                                ptp.update_time(diff);
                            }
                        }

                        ts
                    } else {
                        break;
                    };

                    let polled_ts = dma.poll_timestamp(&packet_id);

                    assert_eq!(polled_ts, Poll::Ready(Ok(Some(rx_timestamp))));

                    *packet_id_ctr += 1;
                    *packet_id_ctr &= !0x8000_0000;
                }

                if let Some((tx_id, sent_time)) = tx_id.take() {
                    if let Poll::Ready(Ok(Some(ts))) = dma.poll_timestamp(&PacketId(tx_id)) {
                        defmt::info!("TX timestamp: {}", ts);
                        defmt::debug!(
                        "Diff between TX timestamp and the time that was put into the packet: {}",
                        ts - sent_time
                    );
                    } else {
                        defmt::warn!("Failed to retrieve TX timestamp");
                    }
                }
            });
    }
}
