use embassy_executor::Spawner;
use embassy_nrf::{
    peripherals::USBD,
    usb::{vbus_detect::SoftwareVbusDetect, Driver},
};
use embassy_sync::channel::TrySendError;
use once_cell::sync::OnceCell;

use shared::device_to_host::DeviceToHost;

use crate::{side, utils::log};
pub use channel::COMMANDS_FROM_HOST;
pub use device::MAX_PACKET_SIZE;

pub mod channel;
pub mod device;
pub mod hid;
pub mod picotool;

mod usb_driver {
    use super::GUESSED_OS;
    use embassy_nrf::{
        peripherals::USBD,
        usb::{vbus_detect::SoftwareVbusDetect, Driver},
    };

    pub type USBDriver = impl embassy_usb::driver::Driver<'static>;

    pub fn set_guesser(driver: Driver<'static, USBD, &'static SoftwareVbusDetect>) -> USBDriver {
        let guesser = embassy_os_guess::OSGuesser::new(|guess| {
            let _ = GUESSED_OS.set(guess);
        });
        guesser.wrap_driver(driver)
    }
}

pub use usb_driver::{set_guesser, USBDriver};

pub static VBUS_DETECT: OnceCell<SoftwareVbusDetect> = OnceCell::new();

static GUESSED_OS: once_cell::sync::OnceCell<embassy_os_guess::OS> =
    once_cell::sync::OnceCell::new();

pub fn guessed_host_os() -> Option<embassy_os_guess::OS> {
    GUESSED_OS.get().copied()
}

pub fn init(spawner: &Spawner, driver: Driver<'static, USBD, &'static SoftwareVbusDetect>) {
    log::info!("Initializing usb");
    let driver = set_guesser(driver);
    let mut builder = device::init_usb(driver);

    channel::init(spawner, &mut builder);
    picotool::init(&mut builder);

    if side::is_master() {
        hid::init(spawner, &mut builder);
    }

    spawner.must_spawn(device::run_usb(builder));
}

pub async fn send_msg(msg: DeviceToHost) {
    if let Err(TrySendError::Full(_msg)) = try_send_msg(msg) {
        // channel::COMMANDS_TO_HOST.send(msg).await;
    }
}

pub fn try_send_msg(msg: DeviceToHost) -> Result<(), TrySendError<DeviceToHost>> {
    channel::COMMANDS_TO_HOST.try_send(msg)
}
