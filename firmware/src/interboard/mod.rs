//! The link between the halves, reliable ordered messages over UDP, see
//! [`glove_net::link`]

use self::channel::PrioritisedMessage;
pub use self::channel::THIS_SIDE_MESSAGE_BUS;
use crate::messages::device_to_device::DeviceToDevice;
use channel::COMMANDS_TO_OTHER_SIDE;
use embassy_executor::Spawner;
use embassy_futures::join::join4;
use embassy_net::{udp::UdpSocket, Stack};
use embassy_time::{Duration, Timer};
use glove_net::link::Link;
use postcard::experimental::max_size::MaxSize;
use rand::RngExt;
use shared::net::{port, Node};

pub mod channel;

/// Whether we're connected to the other side
pub static CONNECTED: crate::sync::Watch<bool> = crate::sync::Watch::new(false);

const PACKET_LEN: usize = DeviceToDevice::POSTCARD_MAX_SIZE + shared::reliable::HEADER_LEN;

pub fn init(spawner: &Spawner, stack: Stack<'static>) {
    spawner.spawn(link_task(stack).unwrap());
}

#[embassy_executor::task]
async fn link_task(stack: Stack<'static>) -> ! {
    let publisher = THIS_SIDE_MESSAGE_BUS.publisher().unwrap();

    let other = if crate::side::is_master() {
        Node::Left
    } else {
        Node::Right
    };
    let peer = (other.link_local(), port::INTERBOARD);

    let mut socket = UdpSocket::new(stack).unwrap();
    // replies go out from our link-local address, as the peer's is
    socket.bind(port::INTERBOARD, peer).unwrap();
    let link = Link::<DeviceToDevice, DeviceToDevice, PACKET_LEN, 4>::new(socket, peer, crate::rng::MyRng.random());

    let monitor = async {
        loop {
            let session = link.wait_up().await;
            crate::log::info!("Connected to the other side");
            CONNECTED.set(true);

            link.wait_down(session).await;
            crate::log::info!("Other side went away");
            CONNECTED.set(false);
            // if the other side rebooted the link is straight back up, give
            // whatever waits on this the chance to see it went down
            Timer::after(Duration::from_millis(10)).await;
        }
    };

    let rx = async {
        loop {
            let msg = link.recv().await;
            crate::log::trace!("Got message: {:?}", msg);
            publisher.publish(msg).await;
        }
    };

    // messages for the other side are dropped while we're not connected, so
    // nothing waiting to send to it gets stuck
    let tx = async {
        loop {
            let msg = COMMANDS_TO_OTHER_SIDE.receive().await.msg;
            if !link.is_up() {
                crate::log::trace!("Not connected, dropping: {:?}", msg);
                continue;
            }

            crate::log::trace!("Sending message: {:?}", msg);
            if link.send(msg).await.is_err() {
                crate::log::info!("Couldn't get a message to the other side");
            }
        }
    };

    join4(link.run(), monitor, rx, tx).await;
    unreachable!()
}

pub async fn send_msg(msg: DeviceToDevice, priority: u8) {
    channel::COMMANDS_TO_OTHER_SIDE
        .send(PrioritisedMessage::new(msg, priority))
        .await;
}

pub fn try_send_msg(msg: DeviceToDevice, priority: u8) -> Result<(), ()> {
    channel::COMMANDS_TO_OTHER_SIDE
        .try_send(PrioritisedMessage::new(msg, priority))
        .map_err(|_e| ())
}
