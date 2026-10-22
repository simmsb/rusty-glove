use self::channel::PrioritisedMessage;
pub use self::channel::THIS_SIDE_MESSAGE_BUS;
use crate::messages::device_to_device::DeviceToDevice;
use channel::COMMANDS_TO_OTHER_SIDE;
use embassy_executor::Spawner;

use crate::ble::BleStack;
pub mod ble;
mod ble_central;
pub mod channel;

pub fn init_central(spawner: &Spawner, stack: &'static BleStack) {
    crate::log::trace!("starting central");
    spawner.spawn(central(stack).unwrap());
}

#[embassy_executor::task]
async fn central(stack: &'static BleStack) {
    let msg_pub = THIS_SIDE_MESSAGE_BUS.publisher().unwrap();
    let rx_fn = || async { COMMANDS_TO_OTHER_SIDE.receive().await.msg };
    let tx_fn = |e| {
        let _ = msg_pub.try_publish(e);
    };

    crate::log::debug!("Starting central bt task");

    ble_central::central_loop(stack, tx_fn, rx_fn).await;
}

pub async fn send_msg(msg: DeviceToDevice, priority: u8) {
    channel::COMMANDS_TO_OTHER_SIDE
        .send(PrioritisedMessage { msg, priority })
        .await;
}

pub fn try_send_msg(msg: DeviceToDevice, priority: u8) -> Result<(), ()> {
    channel::COMMANDS_TO_OTHER_SIDE
        .try_send(PrioritisedMessage { msg, priority })
        .map_err(|_e| ())
}
