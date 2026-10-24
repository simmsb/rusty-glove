//! The right half sends keyboard reports here, and hears back what OS the
//! host is. It's reliable ordered messages over UDP, see [`glove_net::link`].

use embassy_futures::{join::join3, select::select};
use embassy_net::{udp::UdpSocket, Stack};
use embassy_time::{Duration, Timer};
use glove_net::link::{Link, LinkStats};
use postcard::experimental::max_size::MaxSize;
use shared::net::{
    link_packet_len, port, DongleToKeyboard, KeyboardToDongle, Node, KEYBOARD_BATCH,
    KEYBOARD_REPORT_LEN,
};

use crate::usb::{guessed_host_os, KEYBOARD_REPORTS};

const PACKET_LEN: usize = link_packet_len(
    DongleToKeyboard::POSTCARD_MAX_SIZE,
    KeyboardToDongle::POSTCARD_MAX_SIZE,
    KEYBOARD_BATCH,
);
/// Room for a batch to arrive while the last is still being taken
const INBOX: usize = 2 * KEYBOARD_BATCH;

pub static LINK_STATS: LinkStats = LinkStats::new();

/// How often to check whether the OS guess changed
const OS_POLL: Duration = Duration::from_millis(500);

#[embassy_executor::task]
pub async fn keyboard_task(stack: Stack<'static>, epoch: u32) -> ! {
    let peer = (Node::Right.link_local(), port::KEYBOARD);
    let mut socket = UdpSocket::new(stack).unwrap();
    socket.bind(port::KEYBOARD, peer).unwrap();
    let link = Link::<DongleToKeyboard, KeyboardToDongle, PACKET_LEN, INBOX>::new(
        socket,
        peer,
        epoch,
        &LINK_STATS,
    );

    let rx = async {
        loop {
            match link.recv().await {
                KeyboardToDongle::Report(report) => {
                    defmt::trace!("Report: {}", report);
                    KEYBOARD_REPORTS.send(report).await;
                }
            }
        }
    };

    let tx = async {
        loop {
            let session = link.wait_up().await;
            defmt::info!("Keyboard connected");

            let send_os = async {
                let mut sent = None;
                loop {
                    let os = guessed_host_os();
                    if let Some(guess) = os.filter(|_| os != sent) {
                        if link.send(&[DongleToKeyboard::HostOs(guess)]).await.is_ok() {
                            sent = os;
                        }
                    }

                    Timer::after(OS_POLL).await;
                }
            };
            select(link.wait_down(session), send_os).await;
            defmt::info!("Keyboard went away");

            // whatever was held when the keyboard went away would stay held
            KEYBOARD_REPORTS.send([0; KEYBOARD_REPORT_LEN]).await;
        }
    };

    join3(link.run(), rx, tx).await;
    unreachable!()
}
