#![no_std]
#![allow(incomplete_features, async_fn_in_trait)]
#![feature(
    ptr_sub_ptr,
    const_ptr_sub_ptr,
    type_alias_impl_trait,
    impl_trait_in_assoc_type,
    trait_alias,
    maybe_uninit_uninit_array,
    const_maybe_uninit_uninit_array,
    maybe_uninit_array_assume_init,
    const_maybe_uninit_array_assume_init,
    const_maybe_uninit_write,
    const_for,
    async_closure,
    array_chunks
)]

use ble::dfu::DfuConfig;
use embassy_executor::Spawner;
use embassy_nrf::{
    bind_interrupts,
    config::{HfclkSource, LfclkSource},
    gpio::{Input, Level, Output, OutputDrive, Pin, Pull},
    interrupt::InterruptExt,
    usb::vbus_detect::SoftwareVbusDetect,
};
use embassy_sync::{blocking_mutex::raw::ThreadModeRawMutex, mutex::Mutex};
use embassy_time::Timer;
use nrf_softdevice::Softdevice;

#[cfg(feature = "probe")]
use defmt_rtt as _;
#[cfg(not(feature = "reboot_on_panic"))]
use panic_probe as _;
#[cfg(feature = "reboot_on_panic")]
use panic_reset as _;

use usb::VBUS_DETECT;
use utils::log;

use crate::keys::ScannerInstance;

pub mod ble;
mod flash;
pub mod interboard;
pub mod keys;
#[cfg(feature = "logging")]
pub mod logger;
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
async fn softdevice_task(sd: &'static Softdevice) -> ! {
    use nrf_softdevice::SocEvent;
    use nrf_softdevice_s140::NRF_POWER_DCDC_MODES_NRF_POWER_DCDC_ENABLE;

    unsafe {
        nrf_softdevice::raw::sd_power_dcdc_mode_set(
            NRF_POWER_DCDC_MODES_NRF_POWER_DCDC_ENABLE as u8,
        );
        nrf_softdevice::raw::sd_power_dcdc0_mode_set(
            NRF_POWER_DCDC_MODES_NRF_POWER_DCDC_ENABLE as u8,
        );
    }
    unsafe {
        nrf_softdevice::raw::sd_power_usbpwrrdy_enable(1);
        nrf_softdevice::raw::sd_power_usbdetected_enable(1);
        nrf_softdevice::raw::sd_power_usbremoved_enable(1);
    };

    unsafe {
        // always use hfclk, by default SD only turns it on when the radio comes
        // on, but switching over causes jitter on the pwm peripheral...
        nrf_softdevice_s140::sd_clock_hfclk_request();
    }

    let software_vbus = VBUS_DETECT.get().unwrap();

    sd.run_with_callback(|event: SocEvent| {
        log::debug!("SD event: {:?}", event);
        match event {
            SocEvent::PowerUsbRemoved => {
                software_vbus.detected(false);
                state::USB_CONNECTED.set(false);
            }
            SocEvent::PowerUsbDetected => {
                software_vbus.detected(true);
                state::USB_CONNECTED.set(true);
            }
            SocEvent::PowerUsbPowerReady => {
                software_vbus.ready();
                state::USB_CONNECTED.set(true);
            }
            _ => {}
        };
    })
    .await
}

// Keeps our system alive
#[embassy_executor::task]
async fn watchdog_task() {
    let mut handle = unsafe { embassy_nrf::wdt::WatchdogHandle::steal(0) };
    loop {
        handle.pet();
        Timer::after_secs(4).await;
    }
}

bind_interrupts!(struct UsbIrqs {
    USBD => embassy_nrf::usb::InterruptHandler<embassy_nrf::peripherals::USBD>;
});

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
    embassy_nrf::interrupt::POWER_CLOCK.set_priority(embassy_nrf::interrupt::Priority::P2);
    embassy_nrf::interrupt::PWM0.set_priority(embassy_nrf::interrupt::Priority::P2);
    embassy_nrf::interrupt::RNG.set_priority(embassy_nrf::interrupt::Priority::P3);

    let p = embassy_nrf::init(config);

    use nrf_softdevice::ble::set_address;

    let config = nrf_softdevice::Config {
        clock: Some(nrf_softdevice::raw::nrf_clock_lf_cfg_t {
            source: nrf_softdevice::raw::NRF_CLOCK_LF_SRC_XTAL as u8,
            rc_ctiv: 0,
            rc_temp_ctiv: 0,
            accuracy: nrf_softdevice::raw::NRF_CLOCK_LF_ACCURACY_20_PPM as u8,
        }),
        gatts_attr_tab_size: Some(nrf_softdevice::raw::ble_gatts_cfg_attr_tab_size_t {
            attr_tab_size: 32768,
        }),
        conn_gap: Some(nrf_softdevice::raw::ble_gap_conn_cfg_t {
            conn_count: 3,
            event_length: 24,
        }),
        conn_gatt: Some(nrf_softdevice::raw::ble_gatt_conn_cfg_t { att_mtu: 256 }),
        gap_role_count: Some(nrf_softdevice::raw::ble_gap_cfg_role_count_t {
            adv_set_count: 1,
            periph_role_count: 4,
            central_role_count: 4,
            central_sec_count: 1,
            _bitfield_1: nrf_softdevice::raw::ble_gap_cfg_role_count_t::new_bitfield_1(0),
        }),
        gap_device_name: Some(nrf_softdevice::raw::ble_gap_cfg_device_name_t {
            p_value: b"Rusty Glove" as *const u8 as _,
            current_len: 11,
            max_len: 11,
            write_perm: unsafe { core::mem::zeroed() },
            _bitfield_1: nrf_softdevice::raw::ble_gap_cfg_device_name_t::new_bitfield_1(
                nrf_softdevice::raw::BLE_GATTS_VLOC_STACK as u8,
            ),
        }),
        ..Default::default()
    };

    let sd = Softdevice::enable(&config);

    set_address(
        sd,
        &if side::is_master() {
            interboard::ble::CENTRAL_ADDRESS
        } else {
            interboard::ble::PERIPHERAL_ADDRESS
        },
    );

    log::trace!("Configured softdevice");

    // unsafe {
    //     reboot_to_bootloader();
    //     check_bootloader();
    // }

    let flash_mutex = singleton!(Mutex<ThreadModeRawMutex, flash::MkSend<nrf_softdevice::Flash>>,
                                 Mutex::new(flash::MkSend(nrf_softdevice::Flash::take(sd)))
    );

    let dfuconfig = DfuConfig::new(flash_mutex);
    let host_server = ble::make_ble_server(sd);

    if side::is_master() {
        log::trace!("starting central");
        interboard::init_central(&spawner, sd);
    }

    log::trace!("Setting up ble");
    ble::init_peripheral(&spawner, sd, host_server, dfuconfig.clone());

    spawner.must_spawn(softdevice_task(sd));
    spawner.must_spawn(watchdog_task());

    set_status_led(Level::High);

    log::info!("Just a whisper, I hear it in my ghost.");
    log::info!("Side: {}", side::get_side());

    set_status_led(Level::High);

    let vbus_detect = VBUS_DETECT.get_or_init(|| SoftwareVbusDetect::new(true, false));
    let usb_driver = embassy_nrf::usb::Driver::new(p.USBD, UsbIrqs, vbus_detect);

    usb::init(&spawner, usb_driver);

    messages::init(&spawner);

    #[cfg(feature = "logging")]
    logger::init();

    rng::init(sd).await;

    flash::init(flash_mutex).await;

    rgb::init(&spawner, p.PWM0, pins::take_leds!(p).degrade());
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
