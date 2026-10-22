use embassy_sync::{blocking_mutex::raw::ThreadModeRawMutex, channel::Channel};
use packed_struct::PackedStruct;
use trouble_host::prelude::*;
use usbd_human_interface_device::device::keyboard::{
    NKROBootKeyboardReport, NKRO_BOOT_KEYBOARD_REPORT_DESCRIPTOR,
};

use super::Pool;

static KEYBOARD_REPORTS: Channel<ThreadModeRawMutex, NKROBootKeyboardReport, 2> = Channel::new();

pub async fn publish_keyboard_report(report: NKROBootKeyboardReport) {
    crate::log::debug!("kb: {}", defmt::Debug2Format(&report));
    KEYBOARD_REPORTS.send(report).await;
}

type InputReport = <NKROBootKeyboardReport as PackedStruct>::ByteArray;

const REPORT_MAP_LEN: usize = NKRO_BOOT_KEYBOARD_REPORT_DESCRIPTOR.len();
const REPORT_MAP: [u8; REPORT_MAP_LEN] = {
    let mut out = [0u8; REPORT_MAP_LEN];
    let mut i = 0;
    while i < REPORT_MAP_LEN {
        out[i] = NKRO_BOOT_KEYBOARD_REPORT_DESCRIPTOR[i];
        i += 1;
    }
    out
};

#[gatt_service(uuid = service::HUMAN_INTERFACE_DEVICE)]
pub struct HidService {
    #[descriptor(uuid = "2908", read = encrypted, value = [0u8, 1u8])]
    #[characteristic(uuid = "2A4D", read, write, notify, permissions(encrypted))]
    pub input_report: InputReport,

    #[characteristic(uuid = "2A4A", read, value = [0x1, 0x1, 0x0, 0x03], permissions(encrypted))]
    pub hid_info: [u8; 4],

    #[characteristic(uuid = "2A4B", read, value = REPORT_MAP, permissions(encrypted))]
    pub report_map: [u8; REPORT_MAP_LEN],
}

impl HidService {
    pub async fn send_reports(&self, conn: &GattConnection<'_, '_, Pool>) -> ! {
        loop {
            let report = KEYBOARD_REPORTS.receive().await;
            let pack = report.pack().unwrap();

            if let Err(e) = self.input_report.notify(conn, &pack, false).await {
                crate::log::trace!("Failed to send report: {}", defmt::Debug2Format(&e));
            }
        }
    }
}
