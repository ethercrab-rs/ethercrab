//! Common features used in examples.
//!
//! Note that this module isn't an example by itself.

use defmt_rtt as _;
use panic_probe as _;

use stm32_eth::{
    hal::{
        gpio::{GpioExt, *},
        rcc::Clocks,
    },
    EthPins, PartsIn,
};

use fugit::RateExtU32;
use stm32_eth::hal::rcc::RccExt;

/// Setup the clocks and return clocks and a GPIO struct that
/// can be used to set up all of the pins.
///
/// This configures HCLK to be at least 25 MHz, which is the minimum required
/// for ethernet operation to be valid.
pub fn setup_peripherals(p: stm32_eth::stm32::Peripherals) -> (Clocks, Gpio, PartsIn) {
    let ethernet = PartsIn {
        dma: p.ETHERNET_DMA,
        mac: p.ETHERNET_MAC,
        mmc: p.ETHERNET_MMC,
        #[cfg(feature = "ptp")]
        ptp: p.ETHERNET_PTP,
    };

    let rcc = p.RCC.constrain();

    let clocks = rcc.cfgr.sysclk(96.MHz()).hclk(96.MHz());

    #[cfg(feature = "stm32f4xx-hal")]
    let clocks = {
        if cfg!(hse = "bypass") {
            clocks.use_hse(8.MHz()).bypass_hse_oscillator()
        } else if cfg!(hse = "oscillator") {
            clocks.use_hse(8.MHz())
        } else {
            clocks
        }
    };

    #[cfg(feature = "stm32f7xx-hal")]
    let clocks = {
        if cfg!(hse = "bypass") {
            clocks.hse(stm32_eth::hal::rcc::HSEClock::new(
                8.MHz(),
                stm32_eth::hal::rcc::HSEClockMode::Bypass,
            ))
        } else if cfg!(hse = "oscillator") {
            clocks.hse(stm32_eth::hal::rcc::HSEClock::new(
                8.MHz(),
                stm32_eth::hal::rcc::HSEClockMode::Oscillator,
            ))
        } else {
            clocks
        }
    };

    let clocks = clocks.freeze();

    let gpio = Gpio {
        gpioa: p.GPIOA.split(),
        gpiob: p.GPIOB.split(),
        gpioc: p.GPIOC.split(),
        gpiog: p.GPIOG.split(),
    };

    (clocks, gpio, ethernet)
}

pub struct Gpio {
    pub gpioa: gpioa::Parts,
    pub gpiob: gpiob::Parts,
    pub gpioc: gpioc::Parts,
    pub gpiog: gpiog::Parts,
}

pub type RefClk = PA1<Input>;
pub type Crs = PA7<Input>;
pub type TxD1 = PB13<Input>;
pub type RxD0 = PC4<Input>;
pub type RxD1 = PC5<Input>;

#[cfg(not(pins = "nucleo"))]
pub type TxEn = PB11<Input>;
#[cfg(not(pins = "nucleo"))]
pub type TxD0 = PB12<Input>;

#[cfg(pins = "nucleo")]
pub type TxEn = PG11<Input>;
#[cfg(pins = "nucleo")]
pub type TxD0 = PG13<Input>;

pub type Mdio = PA2<Alternate<11>>;
pub type Mdc = PC1<Alternate<11>>;

#[cfg(not(pps = "alternate"))]
pub type Pps = PB5<Output<PushPull>>;

#[cfg(pps = "alternate")]
pub type Pps = PG8<Output<PushPull>>;

pub fn setup_pins(
    gpio: Gpio,
) -> (
    EthPins<RefClk, Crs, TxEn, TxD0, TxD1, RxD0, RxD1>,
    Mdio,
    Mdc,
    Pps,
) {
    #[allow(unused_variables)]
    let Gpio {
        gpioa,
        gpiob,
        gpioc,
        gpiog,
    } = gpio;

    let ref_clk = gpioa.pa1.into_floating_input();
    let crs = gpioa.pa7.into_floating_input();
    let tx_d1 = gpiob.pb13.into_floating_input();
    let rx_d0 = gpioc.pc4.into_floating_input();
    let rx_d1 = gpioc.pc5.into_floating_input();

    #[cfg(not(pins = "nucleo"))]
    let (tx_en, tx_d0) = (
        gpiob.pb11.into_floating_input(),
        gpiob.pb12.into_floating_input(),
    );

    #[cfg(pins = "nucleo")]
    let (tx_en, tx_d0) = {
        (
            gpiog.pg11.into_floating_input(),
            gpiog.pg13.into_floating_input(),
        )
    };

    #[cfg(feature = "stm32f4xx-hal")]
    let (mdio, mdc) = {
        let mut mdio = gpioa.pa2.into_alternate();
        mdio.set_speed(Speed::VeryHigh);
        let mut mdc = gpioc.pc1.into_alternate();
        mdc.set_speed(Speed::VeryHigh);
        (mdio, mdc)
    };

    #[cfg(any(feature = "stm32f7xx-hal"))]
    let (mdio, mdc) = (
        gpioa.pa2.into_alternate().set_speed(Speed::VeryHigh),
        gpioc.pc1.into_alternate().set_speed(Speed::VeryHigh),
    );

    #[cfg(not(pps = "alternate"))]
    let pps = gpiob.pb5.into_push_pull_output();
    #[cfg(pps = "alternate")]
    let pps = gpiog.pg8.into_push_pull_output();

    (
        EthPins {
            ref_clk,
            crs,
            tx_en,
            tx_d0,
            tx_d1,
            rx_d0,
            rx_d1,
        },
        mdio,
        mdc,
        pps,
    )
}

use ieee802_3_miim::{
    phy::{
        lan87xxa::{LAN8720A, LAN8742A},
        BarePhy, KSZ8081R,
    },
    Miim, Pause, Phy,
};

/// An ethernet PHY
pub enum EthernetPhy<M: Miim> {
    /// LAN8720A
    LAN8720A(LAN8720A<M>),
    /// LAN8742A
    LAN8742A(LAN8742A<M>),
    /// KSZ8081R
    KSZ8081R(KSZ8081R<M>),
}

impl<M: Miim> Phy<M> for EthernetPhy<M> {
    fn best_supported_advertisement(&self) -> ieee802_3_miim::AutoNegotiationAdvertisement {
        match self {
            EthernetPhy::LAN8720A(phy) => phy.best_supported_advertisement(),
            EthernetPhy::LAN8742A(phy) => phy.best_supported_advertisement(),
            EthernetPhy::KSZ8081R(phy) => phy.best_supported_advertisement(),
        }
    }

    fn get_miim(&mut self) -> &mut M {
        match self {
            EthernetPhy::LAN8720A(phy) => phy.get_miim(),
            EthernetPhy::LAN8742A(phy) => phy.get_miim(),
            EthernetPhy::KSZ8081R(phy) => phy.get_miim(),
        }
    }

    fn get_phy_addr(&self) -> u8 {
        match self {
            EthernetPhy::LAN8720A(phy) => phy.get_phy_addr(),
            EthernetPhy::LAN8742A(phy) => phy.get_phy_addr(),
            EthernetPhy::KSZ8081R(phy) => phy.get_phy_addr(),
        }
    }
}

impl<M: Miim> EthernetPhy<M> {
    /// Attempt to create one of the known PHYs from the given
    /// MIIM.
    ///
    /// Returns an error if the PHY does not support the extended register
    /// set, or if the PHY's identifier does not correspond to a known PHY.
    pub fn from_miim(miim: M, phy_addr: u8) -> Result<Self, M> {
        let mut bare = BarePhy::new(miim, phy_addr, Pause::NoPause);
        let phy_ident = if let Some(id) = bare.phy_ident() {
            id.raw_u32()
        } else {
            return Err(bare.release());
        };
        let miim = bare.release();
        match phy_ident & 0xFFFFFFF0 {
            0x0007C0F0 => Ok(Self::LAN8720A(LAN8720A::new(miim, phy_addr))),
            0x0007C130 => Ok(Self::LAN8742A(LAN8742A::new(miim, phy_addr))),
            0x00221560 => Ok(Self::KSZ8081R(KSZ8081R::new(miim, phy_addr))),
            _ => Err(miim),
        }
    }

    /// Get a string describing the type of PHY
    pub const fn ident_string(&self) -> &'static str {
        match self {
            EthernetPhy::LAN8720A(_) => "LAN8720A",
            EthernetPhy::LAN8742A(_) => "LAN8742A",
            EthernetPhy::KSZ8081R(_) => "KSZ8081R",
        }
    }

    /// Initialize the PHY
    pub fn phy_init(&mut self) {
        match self {
            EthernetPhy::LAN8720A(phy) => phy.phy_init(),
            EthernetPhy::LAN8742A(phy) => phy.phy_init(),
            EthernetPhy::KSZ8081R(phy) => {
                phy.set_autonegotiation_advertisement(phy.best_supported_advertisement());
            }
        }
    }

    #[allow(dead_code)]
    pub fn speed(&mut self) -> Option<ieee802_3_miim::phy::PhySpeed> {
        match self {
            EthernetPhy::LAN8720A(phy) => phy.link_speed(),
            EthernetPhy::LAN8742A(phy) => phy.link_speed(),
            EthernetPhy::KSZ8081R(phy) => phy.link_speed(),
        }
    }

    #[allow(dead_code)]
    pub fn release(self) -> M {
        match self {
            EthernetPhy::LAN8720A(phy) => phy.release(),
            EthernetPhy::LAN8742A(phy) => phy.release(),
            EthernetPhy::KSZ8081R(phy) => phy.release(),
        }
    }
}
