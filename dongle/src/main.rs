#![no_std]
#![no_main]
#![feature(type_alias_impl_trait, impl_trait_in_assoc_type)]

//! The dongle is the keyboard's way onto a computer. Over USB it's a HID
//! keyboard, passing on the reports the right half sends it over the 802.15.4
//! network, and an ethernet adapter, which hands the host an address over
//! DHCP, takes firmware updates for itself, and forwards the halves' firmware
//! update ports to it.
//!
//! It runs under the same bootloader as the halves, see `memory.x`.

use defmt_rtt as _;
#[cfg(not(feature = "reboot_on_panic"))]
use panic_probe as _;
#[cfg(feature = "reboot_on_panic")]
use panic_reset as _;

use embassy_executor::Spawner;
use embassy_nrf::{
    bind_interrupts,
    interrupt::InterruptExt,
    config::{HfclkSource, LfclkSource},
    peripherals,
    radio::ieee802154::Radio,
    rng::Rng,
    usb::{vbus_detect::HardwareVbusDetect, Driver},
};
use shared::net::{port, Node};

mod dfu;
mod keyboard;
mod net;
mod proxy;
mod usb;

bind_interrupts!(struct Irqs {
    USBD => embassy_nrf::usb::InterruptHandler<peripherals::USBD>;
    CLOCK_POWER => embassy_nrf::usb::vbus_detect::InterruptHandler;
    RADIO => embassy_nrf::radio::InterruptHandler<peripherals::RADIO>;
    RNG => embassy_nrf::rng::InterruptHandler<peripherals::RNG>;
});

#[macro_export]
macro_rules! singleton {
    ($t:ty, $val:expr) => {{
        static STATIC_CELL: ::static_cell::StaticCell<($t,)> = ::static_cell::StaticCell::new();
        let (x,) = STATIC_CELL.init(($val,));
        x
    }};
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let config = {
        let mut config = embassy_nrf::config::Config::default();
        // the radio needs the crystal, there may not be a 32k crystal on a bare board though
        config.hfclk_source = HfclkSource::ExternalXtal;
        config.lfclk_source = LfclkSource::InternalRC;
        config
    };

    let p = embassy_nrf::init(config);

    spawner.spawn(dfu::watchdog_task().unwrap());

    embassy_nrf::interrupt::RADIO.set_priority(embassy_nrf::interrupt::Priority::P1);

    defmt::info!("Dongle starting");

    let (seed, epoch) = {
        let mut rng = Rng::new(p.RNG, Irqs);
        let mut seed = [0; 8];
        rng.fill_bytes(&mut seed).await;
        let mut epoch = [0; 4];
        rng.fill_bytes(&mut epoch).await;
        (u64::from_le_bytes(seed), u32::from_le_bytes(epoch))
    };

    let usb_driver = Driver::new(p.USBD, Irqs, HardwareVbusDetect::new(Irqs));
    let ethernet = usb::init(&spawner, usb_driver);

    let stack = net::init(&spawner, Radio::new(p.RADIO, Irqs), ethernet, seed);

    spawner.spawn(dfu::dfu_task(stack, p.NVMC).unwrap());
    spawner.spawn(keyboard::keyboard_task(stack, epoch).unwrap());
    spawner.spawn(proxy::proxy_task(stack, port::DFU_LEFT, Node::Left).unwrap());
    spawner.spawn(proxy::proxy_task(stack, port::DFU_RIGHT, Node::Right).unwrap());
}
