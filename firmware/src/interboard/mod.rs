use self::channel::PrioritisedMessage;
pub use self::channel::THIS_SIDE_MESSAGE_BUS;
use crate::messages::device_to_device::DeviceToDevice;
use ble_peripheral::SplitServer;
use channel::COMMANDS_TO_OTHER_SIDE;
use embassy_executor::Spawner;
use nrf_softdevice::Softdevice;
pub mod ble;
mod ble_central;
mod ble_peripheral;
pub mod channel;

pub fn make_server(sd: &mut Softdevice) -> SplitServer {
    SplitServer::new(sd).unwrap()
}

pub fn init_peripheral(spawner: &Spawner, sd: &'static Softdevice, server: SplitServer) {
    spawner.must_spawn(peripheral(sd, server));
}

#[embassy_executor::task]
async fn peripheral(sd: &'static Softdevice, server: SplitServer) {
    let msg_pub = THIS_SIDE_MESSAGE_BUS.publisher().unwrap();
    let rx_fn = || async { COMMANDS_TO_OTHER_SIDE.receive().await.msg };
    let tx_fn = |e| {
        let _ = msg_pub.try_publish(e);
    };

    crate::log::debug!("Starting peripheral bt task");

    ble_peripheral::advertisement_loop(sd, &server, tx_fn, rx_fn).await;
}

pub fn init_central(spawner: &Spawner, sd: &'static Softdevice) {
    spawner.must_spawn(central(sd));
}

#[embassy_executor::task]
async fn central(sd: &'static Softdevice) {
    let msg_pub = THIS_SIDE_MESSAGE_BUS.publisher().unwrap();
    let rx_fn = || async { COMMANDS_TO_OTHER_SIDE.receive().await.msg };
    let tx_fn = |e| {
        let _ = msg_pub.try_publish(e);
    };

    crate::log::debug!("Starting central bt task");

    ble_central::central_loop(sd, tx_fn, rx_fn).await;
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
