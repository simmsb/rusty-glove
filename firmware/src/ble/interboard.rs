use embassy_time::Timer;
use nrf_softdevice::ble::Connection;
use postcard::experimental::max_size::MaxSize;

use crate::messages::device_to_device::DeviceToDevice;

#[nrf_softdevice::gatt_service(uuid = "cb6dcd5e-7f1b-11ef-8c55-a71ac707ac76")]
#[derive(Clone)]
pub struct SplitService {
    #[characteristic(uuid = "2e72e4a2-7f1c-11ef-a1c2-13ed5ddd22d6", read, notify)]
    to_central: [u8; DeviceToDevice::POSTCARD_MAX_SIZE],

    #[characteristic(uuid = "3969f044-7f1c-11ef-96c9-7ba31210e355", write_without_response)]
    to_peripheral: [u8; DeviceToDevice::POSTCARD_MAX_SIZE],
}

impl SplitService {
    pub fn process(&self, evt: SplitServiceEvent, mut on_rx: impl FnMut(DeviceToDevice)) {
        match evt {
            SplitServiceEvent::ToCentralCccdWrite { notifications: _ } => {}
            SplitServiceEvent::ToPeripheralWrite(msg) => {
                let Ok(deser) = postcard::from_bytes::<DeviceToDevice>(&msg) else {
                    return;
                };

                crate::log::trace!("Got message: {:?}", deser);

                on_rx(deser);
            }
        }
    }

    pub async fn transmit_loop(
        &self,
        conn: &Connection,
        mut get_tx: impl async FnMut() -> DeviceToDevice,
    ) -> ! {
        loop {
            let msg = get_tx().await;
            let mut buf = [0u8; DeviceToDevice::POSTCARD_MAX_SIZE];
            postcard::to_slice(&msg, &mut buf).unwrap();

            crate::log::trace!("Sending message: {:?}", msg);

            for n in 0u8..20 {
                if self.to_central_notify(conn, &buf).is_ok() {
                    break;
                };

                crate::log::trace!("Failed to send, backing off");

                Timer::after_micros(100 + n as u64 * 500).await;
            }
        }
    }
}
