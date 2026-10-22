//! The link to the USB dongle, which hands our keyboard reports to the host.
//! It's a TCP connection the right half opens to the dongle.

use embassy_executor::Spawner;
use embassy_futures::select::{select, Either};
use embassy_net::{tcp::TcpSocket, Stack};
use embassy_os_guess::OS;
use embassy_sync::{blocking_mutex::raw::ThreadModeRawMutex, channel::Channel};
use embassy_time::{with_timeout, Duration, Timer};
use glove_net::framed::{write_msg, MsgReader};
use packed_struct::PackedStruct;
use portable_atomic::{AtomicU8, Ordering};
use postcard::experimental::max_size::MaxSize;
use shared::net::{
    port, DongleToKeyboard, HostOs, KeyboardToDongle, Node, KEYBOARD_REPORT_LEN,
};
use usbd_human_interface_device::device::keyboard::NKROBootKeyboardReport;

const _: () = assert!(
    core::mem::size_of::<<NKROBootKeyboardReport as PackedStruct>::ByteArray>()
        == KEYBOARD_REPORT_LEN
);

const TX_FRAME_LEN: usize = KeyboardToDongle::POSTCARD_MAX_SIZE + 2;
const RX_FRAME_LEN: usize = DongleToKeyboard::POSTCARD_MAX_SIZE + 2;

const RECONNECT_DELAY: Duration = Duration::from_secs(1);

static KEYBOARD_REPORTS: Channel<ThreadModeRawMutex, NKROBootKeyboardReport, 2> = Channel::new();

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
    let mut rx_buf = [0u8; 256];
    let mut tx_buf = [0u8; 512];

    // reports are snapshots of what's held, so we only need to keep the
    // latest one while we're not connected. Starting with an empty one clears
    // anything the dongle had held from before we rebooted.
    let mut latest = NKROBootKeyboardReport::default();

    loop {
        let mut socket = TcpSocket::new(stack, &mut rx_buf, &mut tx_buf).unwrap();
        glove_net::tune_link_socket(&mut socket, glove_net::LinkEnd::Connecting);

        let connect = socket.connect((Node::Dongle.link_local(), port::KEYBOARD));
        let track = async {
            loop {
                latest = KEYBOARD_REPORTS.receive().await;
            }
        };

        match select(connect, track).await {
            Either::First(Ok(())) => {}
            Either::First(Err(_)) => {
                Timer::after(RECONNECT_DELAY).await;
                continue;
            }
        }

        crate::log::info!("Connected to the dongle");

        let (mut reader, mut writer) = socket.split();

        let tx = async {
            let mut msg = KeyboardToDongle::Report(latest.pack().unwrap());
            loop {
                if let Err(e) = write_msg::<TX_FRAME_LEN, _, _>(&mut writer, &msg).await {
                    return e;
                }

                msg = match select(KEYBOARD_REPORTS.receive(), Timer::after(glove_net::HEARTBEAT))
                    .await
                {
                    Either::First(report) => {
                        latest = report;
                        KeyboardToDongle::Report(report.pack().unwrap())
                    }
                    Either::Second(()) => KeyboardToDongle::Heartbeat,
                };
            }
        };

        let rx = async {
            let mut msgs = MsgReader::<RX_FRAME_LEN>::new();
            loop {
                match msgs.read::<_, DongleToKeyboard>(&mut reader).await {
                    Ok(DongleToKeyboard::HostOs(os)) => set_host_os(os),
                    Err(e) => return e,
                }
            }
        };

        match select(tx, rx).await {
            Either::First(e) => crate::log::info!("Dongle went away: {}", e),
            Either::Second(e) => crate::log::info!("Dongle went away: {}", e),
        }

        socket.abort();
        _ = with_timeout(Duration::from_secs(1), socket.flush()).await;
    }
}

pub fn init(spawner: &Spawner, stack: Stack<'static>) {
    spawner.spawn(link_task(stack).unwrap());
}
