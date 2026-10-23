//! The link between the halves, a TCP connection the left half opens to the
//! right half

use self::channel::PrioritisedMessage;
pub use self::channel::THIS_SIDE_MESSAGE_BUS;
use crate::messages::device_to_device::DeviceToDevice;
use channel::COMMANDS_TO_OTHER_SIDE;
use embassy_executor::Spawner;
use embassy_futures::select::{select, Either};
use embassy_net::{
    tcp::{TcpListener, TcpSocket},
    Stack,
};
use embassy_time::{with_timeout, Duration, Timer};
use glove_net::framed::{write_msg, MsgReader};
use postcard::experimental::max_size::MaxSize;
use shared::net::{port, Node};

pub mod channel;

/// Whether we're connected to the other side
pub static CONNECTED: crate::sync::Watch<bool> = crate::sync::Watch::new(false);

/// Big enough for a COBS encoded message
const FRAME_LEN: usize = DeviceToDevice::POSTCARD_MAX_SIZE + DeviceToDevice::POSTCARD_MAX_SIZE / 254 + 2;

const RECONNECT_DELAY: Duration = Duration::from_secs(1);

pub fn init(spawner: &Spawner, stack: Stack<'static>) {
    spawner.spawn(link_task(stack).unwrap());
}

/// Messages for the other side are dropped while we're not connected, so
/// nothing waiting to send to it gets stuck
async fn drop_messages() -> ! {
    loop {
        let msg = COMMANDS_TO_OTHER_SIDE.receive().await.msg;
        crate::log::trace!("Not connected, dropping: {:?}", msg);
    }
}

#[embassy_executor::task]
async fn link_task(stack: Stack<'static>) -> ! {
    let publisher = THIS_SIDE_MESSAGE_BUS.publisher().unwrap();

    let mut listener = if crate::side::is_master() {
        let mut listener = TcpListener::new(stack).unwrap();
        listener.listen(port::INTERBOARD).unwrap();
        Some(listener)
    } else {
        None
    };

    let mut rx_buf = [0u8; 512];
    let mut tx_buf = [0u8; 512];

    loop {
        let mut socket = TcpSocket::new(stack, &mut rx_buf, &mut tx_buf).unwrap();
        let end = match listener {
            Some(_) => glove_net::LinkEnd::Accepting,
            None => glove_net::LinkEnd::Connecting,
        };
        glove_net::tune_link_socket(&mut socket, end);

        let connect = async {
            match &mut listener {
                Some(listener) => {
                    let token = listener.accept().await.map_err(|_| ())?;
                    socket.accept(token).await.map_err(|_| ())
                }
                None => socket
                    .connect((Node::Right.link_local(), port::INTERBOARD))
                    .await
                    .map_err(|_| ()),
            }
        };

        match select(connect, drop_messages()).await {
            Either::First(Ok(())) => {}
            Either::First(Err(())) => {
                Timer::after(RECONNECT_DELAY).await;
                continue;
            }
        }

        crate::log::info!("Connected to the other side");
        CONNECTED.set(true);

        let (mut reader, mut writer) = socket.split();

        let rx = async {
            let mut msgs = MsgReader::<FRAME_LEN>::new();
            loop {
                let msg = msgs.read::<_, DeviceToDevice>(&mut reader);
                // the connecting side sends something at least every heartbeat
                let msg = match end {
                    glove_net::LinkEnd::Accepting => {
                        match with_timeout(glove_net::LINK_TIMEOUT, msg).await {
                            Ok(msg) => msg,
                            Err(_) => return None,
                        }
                    }
                    glove_net::LinkEnd::Connecting => msg.await,
                };

                match msg {
                    Ok(DeviceToDevice::Heartbeat) => {}
                    Ok(msg) => {
                        crate::log::trace!("Got message: {:?}", msg);
                        publisher.publish(msg).await;
                    }
                    Err(e) => return Some(e),
                }
            }
        };

        let tx = async {
            loop {
                let msg = match end {
                    glove_net::LinkEnd::Connecting => {
                        match with_timeout(glove_net::HEARTBEAT, COMMANDS_TO_OTHER_SIDE.receive())
                            .await
                        {
                            Ok(msg) => msg.msg,
                            Err(_) => DeviceToDevice::Heartbeat,
                        }
                    }
                    glove_net::LinkEnd::Accepting => COMMANDS_TO_OTHER_SIDE.receive().await.msg,
                };

                crate::log::trace!("Sending message: {:?}", msg);
                if let Err(e) = write_msg::<FRAME_LEN, _, _>(&mut writer, &msg).await {
                    return Some(e);
                }
            }
        };

        match select(rx, tx).await {
            Either::First(Some(e)) => crate::log::info!("Other side went away: {}", e),
            Either::Second(Some(e)) => crate::log::info!("Other side went away: {}", e),
            _ => crate::log::info!("Other side went quiet"),
        }

        CONNECTED.set(false);

        socket.abort();
        _ = with_timeout(Duration::from_secs(1), socket.flush()).await;
    }
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
