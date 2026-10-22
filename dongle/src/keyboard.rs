//! The right half connects here to send keyboard reports, and hears back what
//! OS the host is

use embassy_futures::select::{select, Either};
use embassy_net::{
    tcp::{TcpListener, TcpSocket},
    Stack,
};
use embassy_time::{with_timeout, Duration, Timer};
use glove_net::framed::{write_msg, MsgReader};
use postcard::experimental::max_size::MaxSize;
use shared::net::{
    port, DongleToKeyboard, KeyboardToDongle, Node, KEYBOARD_REPORT_LEN,
};

use crate::usb::{guessed_host_os, KEYBOARD_REPORTS};

const RX_FRAME_LEN: usize = KeyboardToDongle::POSTCARD_MAX_SIZE + 2;
const TX_FRAME_LEN: usize = DongleToKeyboard::POSTCARD_MAX_SIZE + 2;

/// How often to check whether the OS guess changed
const OS_POLL: Duration = Duration::from_millis(500);

#[embassy_executor::task]
pub async fn keyboard_task(stack: Stack<'static>) -> ! {
    let mut listener = TcpListener::new(stack).unwrap();
    listener
        .listen((Node::Dongle.link_local(), port::KEYBOARD))
        .unwrap();

    let mut rx_buf = [0u8; 512];
    let mut tx_buf = [0u8; 256];

    loop {
        let Ok(token) = listener.accept().await else {
            continue;
        };

        let mut socket = TcpSocket::new(stack, &mut rx_buf, &mut tx_buf).unwrap();
        glove_net::tune_link_socket(&mut socket, glove_net::LinkEnd::Accepting);
        if socket.accept(token).await.is_err() {
            continue;
        }

        defmt::info!("Keyboard connected");

        let (mut reader, mut writer) = socket.split();

        let rx = async {
            let mut msgs = MsgReader::<RX_FRAME_LEN>::new();
            loop {
                // the keyboard sends something at least every heartbeat
                let msg = with_timeout(
                    glove_net::LINK_TIMEOUT,
                    msgs.read::<_, KeyboardToDongle>(&mut reader),
                )
                .await;

                match msg {
                    Ok(Ok(KeyboardToDongle::Report(report))) => {
                        defmt::trace!("Report: {}", report);
                        KEYBOARD_REPORTS.send(report).await;
                    }
                    Ok(Ok(KeyboardToDongle::Heartbeat)) => {}
                    Ok(Err(e)) => return Some(e),
                    Err(_) => return None,
                }
            }
        };

        let tx = async {
            let mut sent = None;
            loop {
                let os = guessed_host_os();
                if os.is_some() && os != sent {
                    let msg = DongleToKeyboard::HostOs(os.unwrap());
                    if let Err(e) = write_msg::<TX_FRAME_LEN, _, _>(&mut writer, &msg).await {
                        return Some(e);
                    }
                    sent = os;
                }

                Timer::after(OS_POLL).await;
            }
        };

        match select(rx, tx).await {
            Either::First(Some(e)) => defmt::info!("Keyboard went away: {}", e),
            Either::Second(Some(e)) => defmt::info!("Keyboard went away: {}", e),
            _ => defmt::info!("Keyboard went quiet"),
        }

        // whatever was held when the keyboard went away would stay held
        KEYBOARD_REPORTS.send([0; KEYBOARD_REPORT_LEN]).await;

        socket.abort();
        _ = with_timeout(Duration::from_secs(1), socket.flush()).await;
    }
}
