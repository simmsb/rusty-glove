use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use trouble_host::prelude::*;

use super::{device_info::DeviceInformationService, dfu::NrfDfuService, CONNECTIONS_MAX};

#[cfg(feature = "side_right")]
use super::hid::HidService;
#[cfg(not(feature = "side_right"))]
use super::interboard::SplitService;

// TODO battinfo

#[gatt_service(uuid = "c5cef932-96f3-11ef-a546-4374a33721fe")]
pub struct UptimeService {
    #[characteristic(uuid = "caafd50c-96f3-11ef-bf9a-6fd22bab853a", read, notify)]
    pub uptime: u64,
}

// The services differ between the halves, the master (right) side talks HID
// to the host and the other side talks to the master.

#[cfg(feature = "side_right")]
#[gatt_server(
    mutex_type = CriticalSectionRawMutex,
    attribute_table_size = 64,
    connections_max = CONNECTIONS_MAX
)]
pub struct GloveServer {
    pub _dis: DeviceInformationService,
    pub dfu: NrfDfuService,
    pub hid: HidService,
    pub uptime: UptimeService,
}

#[cfg(not(feature = "side_right"))]
#[gatt_server(
    mutex_type = CriticalSectionRawMutex,
    attribute_table_size = 64,
    connections_max = CONNECTIONS_MAX
)]
pub struct GloveServer {
    pub _dis: DeviceInformationService,
    pub dfu: NrfDfuService,
    pub split: SplitService,
    pub uptime: UptimeService,
}

pub fn make_server() -> GloveServer<'static> {
    GloveServer::new_with_config(GapConfig::Peripheral(PeripheralConfig {
        name: "Rusty Glove",
        appearance: &appearance::human_interface_device::KEYBOARD,
    }))
    .unwrap()
}
