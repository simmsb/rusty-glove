use postcard::experimental::max_size::MaxSize;
use trouble_host::prelude::*;

use super::Pool;
use crate::messages::device_to_device::DeviceToDevice;

#[gatt_service(uuid = "cb6dcd5e-7f1b-11ef-8c55-a71ac707ac76")]
pub struct SplitService {
    #[characteristic(uuid = "2e72e4a2-7f1c-11ef-a1c2-13ed5ddd22d6", read, notify)]
    pub to_central: [u8; DeviceToDevice::POSTCARD_MAX_SIZE],

    #[characteristic(uuid = "3969f044-7f1c-11ef-96c9-7ba31210e355", write_without_response)]
    pub to_peripheral: [u8; DeviceToDevice::POSTCARD_MAX_SIZE],
}

impl SplitService {
    pub fn process(&self, msg: &[u8], mut on_rx: impl FnMut(DeviceToDevice)) {
        let Ok(deser) = postcard::from_bytes::<DeviceToDevice>(msg) else {
            return;
        };

        crate::log::trace!("Got message: {:?}", deser);

        on_rx(deser);
    }

    pub async fn transmit_loop(
        &self,
        conn: &GattConnection<'_, '_, Pool>,
        mut get_tx: impl async FnMut() -> DeviceToDevice,
    ) -> ! {
        loop {
            let msg = get_tx().await;
            let mut buf = [0u8; DeviceToDevice::POSTCARD_MAX_SIZE];
            postcard::to_slice(&msg, &mut buf).unwrap();

            crate::log::trace!("Sending message: {:?}", msg);

            if let Err(e) = self.to_central.notify(conn, &buf, false).await {
                crate::log::trace!("Failed to send: {}", defmt::Debug2Format(&e));
            }
        }
    }
}
