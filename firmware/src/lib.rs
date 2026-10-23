#![no_std]
#![allow(incomplete_features, async_fn_in_trait)]
#![feature(
    type_alias_impl_trait,
    impl_trait_in_assoc_type,
    trait_alias,
    maybe_uninit_array_assume_init,
    async_trait_bounds
)]

use dfu::DfuConfig;
use embassy_executor::Spawner;
use embassy_nrf::{
    config::{HfclkSource, LfclkSource},
    gpio::{Input, Level, Output, OutputDrive, Pull},
    interrupt::InterruptExt,
    nvmc::Nvmc,
    radio::ieee802154::Radio,
    usb::vbus_detect::HardwareVbusDetect,
};
use embassy_embedded_hal::adapter::BlockingAsync;
use embassy_sync::{blocking_mutex::raw::ThreadModeRawMutex, mutex::Mutex};
use embassy_time::Timer;

#[cfg(feature = "probe")]
use defmt_rtt as _;
#[cfg(not(feature = "reboot_on_panic"))]
use panic_probe as _;
#[cfg(feature = "reboot_on_panic")]
use panic_reset as _;

use utils::log;

use crate::keys::ScannerInstance;

mod dfu;
mod dongle;
mod flash;
pub mod interboard;
pub mod keys;
pub mod messages;
mod metrics;
mod net;
pub mod pins;
pub mod rgb;
pub mod rng;
pub mod side;
mod sync;
pub mod usb;
pub mod utils;

pub fn set_status_led(_value: Level) {
    // unsafe { ManuallyDrop::new(Output::new(PIN_17::steal(), value)).set_level(value) };
}

pub static VERSION: &str = "0.1.0";

// Keeps our system alive
#[embassy_executor::task]
async fn watchdog_task() {
    log::debug!("Starting watchdog");

    let mut handle =
        unsafe { embassy_nrf::wdt::WatchdogHandle::steal::<embassy_nrf::peripherals::WDT>(0) };
    loop {
        handle.feed();
        Timer::after_secs(4).await;
    }
}

mod ints {
    #![allow(non_local_definitions)]

    use embassy_nrf::bind_interrupts;

    bind_interrupts!(pub struct Irqs {
        USBD => embassy_nrf::usb::InterruptHandler<embassy_nrf::peripherals::USBD>;
        CLOCK_POWER => embassy_nrf::usb::vbus_detect::InterruptHandler;
        RNG => embassy_nrf::rng::InterruptHandler<embassy_nrf::peripherals::RNG>;
        RADIO => embassy_nrf::radio::InterruptHandler<embassy_nrf::peripherals::RADIO>;
    });
}
pub use ints::Irqs;

// bind_interrupts!(struct I2SIrqs {
//     I2S => embassy_nrf::i2s::InterruptHandler<embassy_nrf::peripherals::I2S>;
// });

pub async fn main(spawner: Spawner) {
    log::info!("Early startup notice");

    let config = {
        use embassy_nrf::interrupt::Priority;

        let mut config = embassy_nrf::config::Config::default();
        config.gpiote_interrupt_priority = Priority::P3;
        config.time_interrupt_priority = Priority::P3;
        config.hfclk_source = HfclkSource::ExternalXtal;
        config.lfclk_source = LfclkSource::ExternalXtal;
        config.dcdc.reg0 = true;
        config.dcdc.reg1 = true;

        config
    };

    embassy_nrf::interrupt::USBD.set_priority(embassy_nrf::interrupt::Priority::P2);
    embassy_nrf::interrupt::PWM0.set_priority(embassy_nrf::interrupt::Priority::P2);
    embassy_nrf::interrupt::RADIO.set_priority(embassy_nrf::interrupt::Priority::P1);
    embassy_nrf::interrupt::RNG.set_priority(embassy_nrf::interrupt::Priority::P3);

    // the hfxo is on from here on, the radio needs it and switching over to it
    // causes jitter on the pwm peripheral
    let p = embassy_nrf::init(config);

    let net_seed = {
        let mut rng = embassy_nrf::rng::Rng::new(p.RNG, Irqs);
        rng::init(&mut rng).await
    };

    let flash_mutex = singleton!(Mutex<ThreadModeRawMutex, flash::MkSend<flash::Flash>>,
                                 Mutex::new(flash::MkSend(BlockingAsync::new(Nvmc::new(p.NVMC))))
    );

    let dfuconfig = DfuConfig::from_linkerfile(flash_mutex);

    spawner.spawn(watchdog_task().unwrap());

    set_status_led(Level::High);

    log::info!("Just a whisper, I hear it in my ghost.");
    log::info!("Side: {}", side::get_side());

    set_status_led(Level::High);

    let usb_driver = embassy_nrf::usb::Driver::new(p.USBD, Irqs, HardwareVbusDetect::new(Irqs));

    usb::init(&spawner, usb_driver);

    messages::init(&spawner);

    flash::init(flash_mutex).await;

    let stack = net::init(&spawner, Radio::new(p.RADIO, Irqs), net_seed);
    dfu::init(&spawner, stack, dfuconfig).await;
    interboard::init(&spawner, stack);
    if side::is_master() {
        dongle::init(&spawner, stack);
    }

    rgb::init(&spawner, p.PWM0, pins::take_leds!(p).into());
    // rgb::init(&spawner, p.I2S, pins::take_leds!(p).degrade(), I2SIrqs,
    //           p.P0_20.degrade(), p.P0_21.degrade(),
    //           p.P0_22.degrade()
    // );
    let mut leds_on = Output::new(pins::take_leds_pwr!(p), Level::High, OutputDrive::HighDrive);
    leds_on.set_high();

    let scanner = ScannerInstance::new(
        (
            Output::new(pins::take_col_0!(p), Level::Low, OutputDrive::Standard),
            Output::new(pins::take_col_1!(p), Level::Low, OutputDrive::Standard),
            Output::new(pins::take_col_2!(p), Level::Low, OutputDrive::Standard),
            Output::new(pins::take_col_3!(p), Level::Low, OutputDrive::Standard),
            Output::new(pins::take_col_4!(p), Level::Low, OutputDrive::Standard),
            Output::new(pins::take_col_5!(p), Level::Low, OutputDrive::Standard),
            Output::new(pins::take_col_6!(p), Level::Low, OutputDrive::Standard),
        ),
        (
            Input::new(pins::take_row_0!(p), Pull::Down),
            Input::new(pins::take_row_1!(p), Pull::Down),
            Input::new(pins::take_row_2!(p), Pull::Down),
            Input::new(pins::take_row_3!(p), Pull::Down),
            Input::new(pins::take_row_4!(p), Pull::Down),
            Input::new(pins::take_row_5!(p), Pull::Down),
        ),
    );

    keys::init(&spawner, scanner);

    metrics::init(&spawner).await;

    log::info!("All set up, have fun :)");

    // allowing the main task to exit somehow causes the LED task to break?
    //
    // everything else still works though which is pretty weird
    //
    // anyway, just pend the task forever so it isn't dropped
    core::future::pending::<()>().await;
}
