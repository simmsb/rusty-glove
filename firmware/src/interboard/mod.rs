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
use glove_net::link::{Link, LinkStats};
use postcard::experimental::max_size::MaxSize;
use rand::RngExt;
use shared::net::{port, Node, INTERBOARD_BATCH};

pub mod channel;

/// Whether we're connected to the other side
pub static CONNECTED: crate::sync::Watch<bool> = crate::sync::Watch::new(false);

pub static LINK_STATS: LinkStats = LinkStats::new();

/// How many messages are waiting to go to the other side
pub fn queued() -> usize {
    COMMANDS_TO_OTHER_SIDE.len()
}

const PACKET_LEN: usize =
    shared::reliable::packet_len(DeviceToDevice::POSTCARD_MAX_SIZE, INTERBOARD_BATCH);
/// Room for a batch to arrive while the last is still being taken
const INBOX: usize = 2 * INTERBOARD_BATCH;

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
    let link = Link::<DeviceToDevice, DeviceToDevice, PACKET_LEN, INBOX>::new(
        socket,
        peer,
        crate::rng::MyRng.random(),
        &LINK_STATS,
    );

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
            let batch = glove_net::link::batch::<_, INTERBOARD_BATCH>(
                async { COMMANDS_TO_OTHER_SIDE.receive().await.msg },
                || COMMANDS_TO_OTHER_SIDE.try_receive().ok().map(|m| m.msg),
            )
            .await;

            // keep at it while the link's up: a lost press misses a key press
            // and a lost release sticks a key. Once it's down the other side
            // lets go of what we held.
            loop {
                if !link.is_up() {
                    crate::log::trace!("Not connected, dropping: {:?}", batch);
                    break;
                }

                crate::log::trace!("Sending messages: {:?}", batch);
                match link.send(&batch).await {
                    Ok(()) => break,
                    Err(_) => crate::log::info!("Still trying to get messages to the other side"),
                }
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
