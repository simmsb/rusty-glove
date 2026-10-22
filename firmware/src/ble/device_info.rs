use trouble_host::prelude::*;

#[repr(u8)]
#[derive(Clone, Copy)]
pub enum VidSource {
    BluetoothSIG = 1,
    #[allow(unused)]
    UsbIF = 2,
}

pub struct PnPID {
    pub vid_source: VidSource,
    pub vendor_id: u16,
    pub product_id: u16,
    pub product_version: u16,
}

impl PnPID {
    pub const fn to_bytes(&self) -> [u8; 7] {
        let vid = self.vendor_id.to_le_bytes();
        let pid = self.product_id.to_le_bytes();
        let ver = self.product_version.to_le_bytes();
        [
            self.vid_source as u8,
            vid[0],
            vid[1],
            pid[0],
            pid[1],
            ver[0],
            ver[1],
        ]
    }
}

const PNP_ID: [u8; 7] = PnPID {
    vid_source: VidSource::BluetoothSIG,
    vendor_id: 0x8192,
    product_id: 0x4096,
    product_version: 1,
}
.to_bytes();

#[gatt_service(uuid = service::DEVICE_INFORMATION)]
pub struct DeviceInformationService {
    #[characteristic(uuid = characteristic::PNP_ID, read, value = PNP_ID)]
    pub pnp_id: [u8; 7],

    #[characteristic(uuid = characteristic::MANUFACTURER_NAME_STRING, read, value = "Ben Simms")]
    pub manufacturer_name: &'static str,

    #[characteristic(uuid = characteristic::MODEL_NUMBER_STRING, read, value = "1234")]
    pub model_number: &'static str,

    #[characteristic(uuid = characteristic::SERIAL_NUMBER_STRING, read, value = "0001")]
    pub serial_number: &'static str,

    #[characteristic(uuid = characteristic::HARDWARE_REVISION_STRING, read, value = "glove")]
    pub hw_rev: &'static str,

    #[characteristic(
        uuid = characteristic::FIRMWARE_REVISION_STRING,
        read,
        value = build_time::build_time_utc!()
    )]
    pub fw_rev: &'static str,
}
