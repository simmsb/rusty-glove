use embassy_sync::{blocking_mutex::raw::ThreadModeRawMutex, channel::Channel};
use nrf_softdevice::ble::Connection;
use packed_struct::PackedStruct;
use usbd_human_interface_device::device::keyboard::{
    NKROBootKeyboardReport, NKRO_BOOT_KEYBOARD_REPORT_DESCRIPTOR,
};

static KEYBOARD_REPORTS: Channel<ThreadModeRawMutex, NKROBootKeyboardReport, 2> = Channel::new();

pub async fn publish_keyboard_report(report: NKROBootKeyboardReport) {
    crate::log::debug!("kb: {}", defmt::Debug2Format(&report));
    KEYBOARD_REPORTS.send(report).await;
}

#[nrf_softdevice::gatt_service(uuid = "1812")]
#[derive(Clone)]
pub struct HidService {
    // If you have multiple descriptors, just add them all
    #[characteristic(
        uuid = "2A4D",
        security = "justworks",
        read,
        write,
        notify,
        descriptor(uuid = "2908", security = "justworks", value = "[0, 1]")
    )]
    pub input_report: <NKROBootKeyboardReport as PackedStruct>::ByteArray,

    #[characteristic(
        uuid = "2A4A",
        security = "justworks",
        read,
        value = "[0x1, 0x1, 0x0, 0x03]"
    )]
    pub hid_info: u8,

    #[characteristic(
        uuid = "2A4B",
        security = "justworks",
        read,
        value = "NKRO_BOOT_KEYBOARD_REPORT_DESCRIPTOR"
    )]
    pub report_map: [u8; NKRO_BOOT_KEYBOARD_REPORT_DESCRIPTOR.len()],
}

impl HidService {
    pub async fn send_reports(&self, conn: &Connection) -> ! {
        loop {
            let report = KEYBOARD_REPORTS.receive().await;

            let _ = self.input_report_notify(conn, &report.pack().unwrap());
        }
    }
}
