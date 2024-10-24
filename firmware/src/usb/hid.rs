use embassy_executor::Spawner;
use embassy_sync::channel::Channel;
use embassy_usb::{class::hid::HidWriter, Builder};
use packed_struct::PackedStruct;
use usbd_human_interface_device::device::keyboard::{
    NKROBootKeyboardReport, NKRO_BOOT_KEYBOARD_REPORT_DESCRIPTOR,
};

use crate::utils;

use super::USBDriver;

type CS = embassy_sync::blocking_mutex::raw::ThreadModeRawMutex;

static KEYBOARD_REPORTS: Channel<CS, NKROBootKeyboardReport, 2> = Channel::new();

// pub async fn publish_keyboard_report(report: NKROBootKeyboardReport) {
//     crate::log::debug!("kb: {}", defmt::Debug2Format(&report));
//     KEYBOARD_REPORTS.send(report).await;
// }

#[embassy_executor::task]
async fn keyboard_writer(mut keyboard_writer: HidWriter<'static, USBDriver, 64>) {
    loop {
        let report = KEYBOARD_REPORTS.receive().await;
        let _ = keyboard_writer.write(&report.pack().unwrap()).await;
    }
}
pub fn init(spawner: &Spawner, builder: &mut Builder<'static, USBDriver>) {
    let keyboard_state = utils::singleton!(
        embassy_usb::class::hid::State,
        embassy_usb::class::hid::State::new()
    );

    let keyboard_hid_writer = HidWriter::new(
        builder,
        keyboard_state,
        embassy_usb::class::hid::Config {
            report_descriptor: NKRO_BOOT_KEYBOARD_REPORT_DESCRIPTOR,
            request_handler: None,
            poll_ms: 2,
            max_packet_size: 64,
        },
    );

    spawner.must_spawn(keyboard_writer(keyboard_hid_writer));
}
