#![no_std]
#![allow(incomplete_features, async_fn_in_trait)]
#![feature(
    type_alias_impl_trait,
    impl_trait_in_assoc_type,
    trait_alias,
    maybe_uninit_array_assume_init,
    async_trait_bounds
)]

use ble::dfu::DfuConfig;
use embassy_executor::Spawner;
use embassy_nrf::{
    config::{HfclkSource, LfclkSource},
    gpio::{Input, Level, Output, OutputDrive, Pull},
    interrupt::InterruptExt,
    mode::Async,
    usb::vbus_detect::HardwareVbusDetect,
};
use embassy_sync::{blocking_mutex::raw::ThreadModeRawMutex, mutex::Mutex};
use embassy_time::Timer;
use nrf_sdc::mpsl::{self, MultiprotocolServiceLayer};

#[cfg(feature = "probe")]
use defmt_rtt as _;
#[cfg(not(feature = "reboot_on_panic"))]
use panic_probe as _;
#[cfg(feature = "reboot_on_panic")]
use panic_reset as _;

use utils::log;

use crate::keys::ScannerInstance;

pub mod ble;
mod flash;
pub mod interboard;
pub mod keys;
pub mod messages;
mod metrics;
pub mod pins;
pub mod rgb;
pub mod rng;
pub mod side;
pub mod state;
mod sync;
pub mod usb;
pub mod utils;

pub fn set_status_led(_value: Level) {
    // unsafe { ManuallyDrop::new(Output::new(PIN_17::steal(), value)).set_level(value) };
}

pub static VERSION: &str = "0.1.0";

#[embassy_executor::task]
async fn mpsl_task(mpsl: &'static MultiprotocolServiceLayer<'static>) -> ! {
    mpsl.run().await
}

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
        CLOCK_POWER => nrf_sdc::mpsl::ClockInterruptHandler, embassy_nrf::usb::vbus_detect::InterruptHandler;
        RNG => embassy_nrf::rng::InterruptHandler<embassy_nrf::peripherals::RNG>;
        EGU0_SWI0 => nrf_sdc::mpsl::LowPrioInterruptHandler;
        RADIO => nrf_sdc::mpsl::HighPrioInterruptHandler;
        TIMER0 => nrf_sdc::mpsl::HighPrioInterruptHandler;
        RTC0 => nrf_sdc::mpsl::HighPrioInterruptHandler;
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

    // the MPSL sets the priorities of the radio and clock interrupts itself
    embassy_nrf::interrupt::USBD.set_priority(embassy_nrf::interrupt::Priority::P2);
    embassy_nrf::interrupt::PWM0.set_priority(embassy_nrf::interrupt::Priority::P2);
    embassy_nrf::interrupt::RNG.set_priority(embassy_nrf::interrupt::Priority::P3);

    let p = embassy_nrf::init(config);

    let mpsl = {
        let mpsl_p =
            mpsl::Peripherals::new(p.RTC0, p.TIMER0, p.TEMP, p.PPI_CH19, p.PPI_CH30, p.PPI_CH31);
        let lfclk_cfg = mpsl::raw::mpsl_clock_lfclk_cfg_t {
            source: mpsl::raw::MPSL_CLOCK_LF_SRC_XTAL as u8,
            rc_ctiv: 0,
            rc_temp_ctiv: 0,
            accuracy_ppm: 20,
            skip_wait_lfclk_started: false,
        };
        let session_mem = singleton!(mpsl::SessionMem<1>, mpsl::SessionMem::new());

        singleton!(
            MultiprotocolServiceLayer<'static>,
            MultiprotocolServiceLayer::with_timeslots(mpsl_p, Irqs, lfclk_cfg, session_mem)
                .unwrap()
        )
    };
    spawner.spawn(mpsl_task(mpsl).unwrap());

    // always keep the hfxo running, by default the MPSL only turns it on when
    // the radio comes on, but switching over causes jitter on the pwm
    // peripheral...
    core::mem::forget(mpsl.request_hfclk().await.unwrap());

    log::trace!("Configured MPSL");

    let rng = singleton!(
        embassy_nrf::rng::Rng<'static, Async>,
        embassy_nrf::rng::Rng::new(p.RNG, Irqs)
    );
    rng::init(rng).await;

    let sdc = ble::make_controller(
        nrf_sdc::Peripherals::new(
            p.PPI_CH17, p.PPI_CH18, p.PPI_CH20, p.PPI_CH21, p.PPI_CH22, p.PPI_CH23, p.PPI_CH24,
            p.PPI_CH25, p.PPI_CH26, p.PPI_CH27, p.PPI_CH28, p.PPI_CH29,
        ),
        rng,
        mpsl,
    );

    let flash_mutex = singleton!(Mutex<ThreadModeRawMutex, flash::MkSend<flash::Flash>>,
                                 Mutex::new(flash::MkSend(flash::Flash::take(mpsl, p.NVMC)))
    );

    let dfuconfig = DfuConfig::new(flash_mutex);

    spawner.spawn(watchdog_task().unwrap());

    set_status_led(Level::High);

    log::info!("Just a whisper, I hear it in my ghost.");
    log::info!("Side: {}", side::get_side());

    set_status_led(Level::High);

    let usb_driver = embassy_nrf::usb::Driver::new(p.USBD, Irqs, HardwareVbusDetect::new(Irqs));

    usb::init(&spawner, usb_driver);

    messages::init(&spawner);

    flash::init(flash_mutex).await;

    // the bonds are stored in flash, so bring the ble stack up after the database
    ble::init(&spawner, sdc, dfuconfig.clone()).await;

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

    {
        // wait a bit before marking good firmware
        Timer::after_secs(2).await;

        let mut magic = embassy_boot::AlignedBuffer([0; 4]);
        let mut state = embassy_boot_nrf::FirmwareState::new(dfuconfig.state(), &mut magic.0);

        crate::log::debug!(
            "Current firmware state: {}",
            defmt::Debug2Format(&state.get_state().await)
        );

        if let Err(e) = state.mark_booted().await {
            crate::log::error!(
                "Failed to mark successful booting (wed'll roll back on reboot): {}",
                defmt::Debug2Format(&e)
            )
        }
    }

    // allowing the main task to exit somehow causes the LED task to break?
    //
    // everything else still works though which is pretty weird
    //
    // anyway, just pend the task forever so it isn't dropped
    core::future::pending::<()>().await;
}
