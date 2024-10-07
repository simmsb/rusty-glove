use nrf_softdevice::{ble::gatt_server::Server, Softdevice};

use super::device_info::{DeviceInformation, DeviceInformationService, PnPID};

// TODO battinfo
// TODO fwupd

pub struct NonHIDServer {
    _dis: DeviceInformationService,
}

impl NonHIDServer {
    pub fn new(sd: &mut Softdevice) -> Self {
        let dis = device_info(sd, None);

        Self { _dis: dis }
    }
}

impl Server for NonHIDServer {
    type Event = ();

    fn on_write(
        &self,
        _conn: &nrf_softdevice::ble::Connection,
        handle: u16,
        _op: nrf_softdevice::ble::gatt_server::WriteOp,
        _offset: usize,
        data: &[u8],
    ) -> Option<Self::Event> {
        None
    }
}

fn device_info(sd: &mut Softdevice, pnp: Option<&PnPID>) -> DeviceInformationService {
    DeviceInformationService::new(
        sd,
        pnp,
        DeviceInformation {
            manufacturer_name: Some("Ben Simms"),
            model_number: Some("1234"),
            serial_number: Some("0001"),
            hw_rev: Some("glove"),
            fw_rev: Some("fill me in"),
            sw_rev: None,
        },
    )
    .unwrap()
}