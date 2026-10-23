//! Firmware updates from the host, on our USB side, see [`glove_net::dfu`]
//!
//! We run under the same bootloader as the halves, which starts the watchdog,
//! so it's fed here too.

use embassy_embedded_hal::adapter::BlockingAsync;
use embassy_net::Stack;
use embassy_nrf::{nvmc::Nvmc, peripherals, wdt::WatchdogHandle, Peri};
use embassy_sync::{blocking_mutex::raw::NoopRawMutex, mutex::Mutex};
use embassy_time::Timer;
use glove_net::dfu::DfuConfig;
use shared::net::{port, usb as usb_net};

use crate::singleton;

#[embassy_executor::task]
pub async fn dfu_task(stack: Stack<'static>, nvmc: Peri<'static, peripherals::NVMC>) -> ! {
    let flash = singleton!(
        Mutex<NoopRawMutex, BlockingAsync<Nvmc<'static>>>,
        Mutex::new(BlockingAsync::new(Nvmc::new(nvmc)))
    );
    let config = DfuConfig::from_linkerfile(flash);

    glove_net::dfu::check_boot(&config).await;
    glove_net::dfu::serve(stack, (usb_net::DONGLE_IP, port::DFU), config).await
}

#[embassy_executor::task]
pub async fn watchdog_task() -> ! {
    // the bootloader set it going with a minute's timeout
    let mut handle = unsafe { WatchdogHandle::steal::<peripherals::WDT>(0) };
    loop {
        handle.feed();
        Timer::after_secs(4).await;
    }
}
