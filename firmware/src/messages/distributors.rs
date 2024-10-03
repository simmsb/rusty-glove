use shared::device_to_host::{DeviceToHost, DeviceToHostMsg};
use shared::host_to_device::HostToDeviceMsg;

use crate::side;
use crate::{interboard, usb};

use super::device_to_device::DeviceToDevice;

#[embassy_executor::task]
pub async fn from_usb_distributor() {
    let mut sub = crate::usb::COMMANDS_FROM_HOST.subscriber().unwrap();

    loop {
        let msg = sub.next_message_pure().await;

        if msg.targets_side(side::get_side()) {
            #[allow(unreachable_code)]
            handle_from_host(msg.msg.clone()).await;
        }
        if msg.targets_side(side::get_other_side()) {
            interboard::send_msg(DeviceToDevice::ForwardedFromHost(msg.msg), 2).await;
        }
    }
}

async fn handle_from_host(msg: HostToDeviceMsg) {
    match msg {}
}

#[embassy_executor::task]
pub async fn from_other_side_distributor() {
    let mut sub = crate::interboard::THIS_SIDE_MESSAGE_BUS
        .subscriber()
        .unwrap();

    loop {
        let msg = sub.next_message_pure().await;
        // crate::log::info!("got msg: {:?}", msg);

        match msg {
            DeviceToDevice::Ping => {
                // log::info!("Got a ping");
                interboard::send_msg(DeviceToDevice::Pong, 3).await;
            }
            DeviceToDevice::Pong => {
                // log::info!("Got a pong");
            }
            DeviceToDevice::ForwardedToHost(msg) => {
                usb::send_msg(msg).await;
            }
            DeviceToDevice::ForwardedFromHost(msg) => {
                handle_from_host(msg).await;
            }
            _ => {}
        }
    }
}

#[derive(PartialEq, Eq)]
pub enum MessageProvenance {
    Origin,
    Forwarded,
}

pub async fn send_to_host(msg: DeviceToHostMsg, provenance: MessageProvenance) {
    let side = side::get_side();
    let msg = DeviceToHost {
        from_side: side,
        msg,
    };
    if side::is_master() {
        usb::send_msg(msg).await;
    } else if provenance == MessageProvenance::Origin {
        let msg = DeviceToDevice::ForwardedToHost(msg);
        interboard::send_msg(msg, 3).await;
    }
}

pub fn try_send_to_host(msg: DeviceToHostMsg, provenance: MessageProvenance) -> Option<()> {
    let side = side::get_side();
    let msg = DeviceToHost {
        from_side: side,
        msg,
    };
    if side::is_master() {
        usb::try_send_msg(msg).ok()
    } else if provenance == MessageProvenance::Origin {
        let msg = DeviceToDevice::ForwardedToHost(msg);
        interboard::try_send_msg(msg, 3).ok()
    } else {
        // if we get here it means both sides have no usb connection
        Some(())
    }
}
