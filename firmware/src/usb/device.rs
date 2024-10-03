use embassy_usb::driver::Driver;
use embassy_usb::{Builder, Config};

use crate::utils::singleton;

use super::USBDriver;

pub const MAX_PACKET_SIZE: u16 = 64;

pub fn init_usb<'d, D: Driver<'d>>(driver: D) -> Builder<'d, D> {
    let mut config = Config::new(0x2e8a, 0x000a);
    config.manufacturer = Some("Ben Simms");
    config.product = Some("Glove80");
    config.serial_number = None;
    config.max_power = 500;
    config.max_packet_size_0 = MAX_PACKET_SIZE as u8;

    Builder::new(
        driver,
        config,
        singleton!([u8; 256], [0; 256]),
        singleton!([u8; 256], [0; 256]),
        singleton!([u8; 256], [0; 256]),
        singleton!([u8; 256], [0; 256]),
    )
}

#[embassy_executor::task]
pub async fn run_usb(builder: Builder<'static, USBDriver>) {
    let mut device = builder.build();
    device.run().await;
}
