use embassy_executor::Spawner;

pub mod device_to_device;
pub mod distributors;
pub mod transmissions;

pub use distributors::{send_to_host, try_send_to_host};

pub fn init(spawner: &Spawner) {
    spawner.must_spawn(distributors::from_usb_distributor());
    spawner.must_spawn(distributors::from_other_side_distributor());
}
