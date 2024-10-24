use core::ops::Not;

use super::{
    device_info::{DeviceInformation, DeviceInformationService, PnPID},
    dfu::NrfDfuService,
    hid::HidService,
    interboard::SplitService,
};
use nrf_softdevice::{
    ble::gatt_server::{Server, Service},
    Softdevice,
};

// TODO battinfo

#[derive(Clone)]
pub struct GloveServer {
    _dis: DeviceInformationService,
    pub dfu: NrfDfuService,
    pub hid: Option<HidService>,
    pub split: Option<SplitService>,
}

impl GloveServer {
    pub fn new(sd: &mut Softdevice) -> Self {
        let pnp = crate::side::is_master().then(|| &PnPID {
            vid_source: super::device_info::VidSource::BluetoothSIG,
            vendor_id: 0x8192,
            product_id: 0x4096,
            product_version: 1,
        });

        let dis = device_info(sd, pnp);
        let dfu = NrfDfuService::new(sd).unwrap();
        let hid = crate::side::is_master().then(|| HidService::new(sd).unwrap());
        let split = crate::side::is_master()
            .not()
            .then(|| SplitService::new(sd).unwrap());

        Self {
            _dis: dis,
            dfu,
            hid,
            split,
        }
    }
}

pub enum GloveServerEvent {
    DFU(<NrfDfuService as Service>::Event),
    HID(<HidService as Service>::Event),
    Split(<SplitService as Service>::Event),
}

impl Server for GloveServer {
    type Event = GloveServerEvent;

    fn on_write(
        &self,
        _conn: &nrf_softdevice::ble::Connection,
        handle: u16,
        _op: nrf_softdevice::ble::gatt_server::WriteOp,
        _offset: usize,
        data: &[u8],
    ) -> Option<Self::Event> {
        if let Some(e) = self.dfu.on_write(handle, data) {
            // match e {
            //     super::dfu::NrfDfuServiceEvent::ControlCccdWrite { .. }
            //     | super::dfu::NrfDfuServiceEvent::PacketCccdWrite { .. } => {
            //         if let Some(bonder) = BONDER.get() {
            //             bonder.save_sys_attrs(conn);
            //         }
            //     }
            //     _ => {}
            // };
            return Some(GloveServerEvent::DFU(e));
        }

        if let Some(e) = self.hid.as_ref().and_then(|x| x.on_write(handle, data)) {
            return Some(GloveServerEvent::HID(e));
        }

        if let Some(e) = self.split.as_ref().and_then(|x| x.on_write(handle, data)) {
            return Some(GloveServerEvent::Split(e));
        }

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
            fw_rev: Some(build_time::build_time_utc!()),
            sw_rev: None,
        },
    )
    .unwrap()
}
