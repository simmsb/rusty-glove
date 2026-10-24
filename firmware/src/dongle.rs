//! The link to the USB dongle, which hands our keyboard reports to the host.
//! It's reliable ordered messages over UDP, see [`glove_net::link`].

use embassy_executor::Spawner;
use embassy_futures::{
    join::join3,
    select::{select, Either},
};
use embassy_net::{udp::UdpSocket, Stack};
use embassy_os_guess::OS;
use embassy_sync::{blocking_mutex::raw::ThreadModeRawMutex, channel::Channel};
use glove_net::link::{Link, LinkStats};
use packed_struct::PackedStruct;
use portable_atomic::{AtomicU8, Ordering};
use postcard::experimental::max_size::MaxSize;
use rand::RngExt;
use shared::net::{
    port, DongleToKeyboard, HostOs, KeyboardToDongle, Node, KEYBOARD_BATCH, KEYBOARD_REPORT_LEN,
};
use usbd_human_interface_device::device::keyboard::NKROBootKeyboardReport;

const _: () = assert!(
    core::mem::size_of::<<NKROBootKeyboardReport as PackedStruct>::ByteArray>()
        == KEYBOARD_REPORT_LEN
);

const PACKET_LEN: usize = shared::net::link_packet_len(
    KeyboardToDongle::POSTCARD_MAX_SIZE,
    DongleToKeyboard::POSTCARD_MAX_SIZE,
    KEYBOARD_BATCH,
);
/// Room for a batch to arrive while the last is still being taken
const INBOX: usize = 2 * KEYBOARD_BATCH;
// a full batch goes in one radio frame, see `shared::reliable`'s tests
const _: () = assert!(PACKET_LEN <= 90);

/// Every report matters, a press and release that only show up in reports
/// that are dropped never make it to the host. While this is full the key
/// processing waits, so it must ride out the link being slow for a bit.
static KEYBOARD_REPORTS: Channel<ThreadModeRawMutex, NKROBootKeyboardReport, 32> =
    Channel::new();

pub static LINK_STATS: LinkStats = LinkStats::new();

/// How many reports are waiting to go to the dongle
pub fn queued() -> usize {
    KEYBOARD_REPORTS.len()
}

pub async fn publish_keyboard_report(report: NKROBootKeyboardReport) {
    crate::log::debug!("kb: {}", defmt::Debug2Format(&report));
    KEYBOARD_REPORTS.send(report).await;
}

const OS_UNKNOWN: u8 = u8::MAX;
static HOST_OS: AtomicU8 = AtomicU8::new(OS_UNKNOWN);

/// The OS the dongle thinks it's plugged into
pub fn host_os() -> Option<OS> {
    Some(match HOST_OS.load(Ordering::Relaxed) {
        0 => OS::Windows,
        1 => OS::Linux,
        2 => OS::MacOS,
        3 => OS::Unknown,
        _ => return None,
    })
}

fn set_host_os(os: HostOs) {
    let v = match os {
        HostOs::Windows => 0,
        HostOs::Linux => 1,
        HostOs::MacOS => 2,
        HostOs::Unknown => 3,
    };
    HOST_OS.store(v, Ordering::Relaxed);
}

#[embassy_executor::task]
async fn link_task(stack: Stack<'static>) -> ! {
    let peer = (Node::Dongle.link_local(), port::KEYBOARD);
    let mut socket = UdpSocket::new(stack).unwrap();
    socket.bind(port::KEYBOARD, peer).unwrap();
    let link = Link::<KeyboardToDongle, DongleToKeyboard, PACKET_LEN, INBOX>::new(
        socket,
        peer,
        crate::rng::MyRng.random(),
        &LINK_STATS,
    );

    let rx = async {
        loop {
            match link.recv().await {
                DongleToKeyboard::HostOs(os) => set_host_os(os),
            }
        }
    };

    let tx = async {
        // reports are snapshots of what's held, so we only need to keep the
        // latest one while the dongle's away. Starting with an empty one
        // clears anything the dongle had held from before we rebooted.
        let mut latest = NKROBootKeyboardReport::default();

        loop {
            let track = async {
                loop {
                    latest = KEYBOARD_REPORTS.receive().await;
                }
            };
            let session = match select(link.wait_up(), track).await {
                Either::First(session) => session,
            };
            crate::log::info!("Connected to the dongle");

            let send = async {
                let mut reports = heapless::Vec::<_, KEYBOARD_BATCH>::new();
                _ = reports.push(latest);
                loop {
                    let msgs = heapless::Vec::<_, KEYBOARD_BATCH>::from_iter(
                        reports
                            .iter()
                            .map(|r| KeyboardToDongle::Report(r.pack().unwrap())),
                    );
                    match link.send(&msgs).await {
                        Ok(()) => {
                            reports = glove_net::link::batch(KEYBOARD_REPORTS.receive(), || {
                                KEYBOARD_REPORTS.try_receive().ok()
                            })
                            .await;
                            // what to start with if the dongle goes away
                            // before this gets there
                            latest = *reports.last().unwrap();
                        }
                        // try the same ones again, skipping one could lose
                        // a key press
                        Err(_) => crate::log::info!("Still trying to get reports to the dongle"),
                    }
                }
            };
            select(link.wait_down(session), send).await;
            crate::log::info!("Dongle went away");
        }
    };

    join3(link.run(), rx, tx).await;
    unreachable!()
}

pub fn init(spawner: &Spawner, stack: Stack<'static>) {
    spawner.spawn(link_task(stack).unwrap());
}
