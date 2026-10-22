use embassy_executor::Spawner;
use embassy_nrf::usb::{vbus_detect::HardwareVbusDetect, Driver};
use embassy_sync::channel::TrySendError;

use shared::device_to_host::DeviceToHost;

use crate::{side, utils::log};
pub use channel::COMMANDS_FROM_HOST;
pub use device::MAX_PACKET_SIZE;

pub mod channel;
pub mod device;
pub mod hid;

mod usb_driver {
    use super::GUESSED_OS;
    use embassy_nrf::usb::{vbus_detect::HardwareVbusDetect, Driver};

    pub type USBDriver = impl embassy_usb::driver::Driver<'static>;

    #[define_opaque(USBDriver)]
    pub fn set_guesser(driver: Driver<'static, HardwareVbusDetect>) -> USBDriver {
        let guesser = embassy_os_guess::OSGuesser::new(|guess| {
            let _ = GUESSED_OS.set(guess);
        });
        guesser.wrap_driver(driver)
    }
}

pub use usb_driver::{set_guesser, USBDriver};

static GUESSED_OS: once_cell::sync::OnceCell<embassy_os_guess::OS> =
    once_cell::sync::OnceCell::new();

/// Prefers what the dongle saw, as that's what we're typing into
pub fn guessed_host_os() -> Option<embassy_os_guess::OS> {
    crate::dongle::host_os().or_else(|| GUESSED_OS.get().copied())
}

pub fn init(spawner: &Spawner, driver: Driver<'static, HardwareVbusDetect>) {
    log::info!("Initializing usb");
    let driver = set_guesser(driver);
    let mut builder = device::init_usb(driver);

    channel::init(spawner, &mut builder);

    if side::is_master() {
        hid::init(spawner, &mut builder);
    }

    spawner.spawn(device::run_usb(builder).unwrap());
}

pub async fn send_msg(msg: DeviceToHost) {
    if let Err(TrySendError::Full(_msg)) = try_send_msg(msg) {
        // channel::COMMANDS_TO_HOST.send(msg).await;
    }
}

pub fn try_send_msg(msg: DeviceToHost) -> Result<(), TrySendError<DeviceToHost>> {
    channel::COMMANDS_TO_HOST.try_send(msg)
}
