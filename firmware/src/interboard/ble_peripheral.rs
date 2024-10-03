use nrf_softdevice::{
    ble::{
        gatt_server::run,
        peripheral::{advertise_connectable, ConnectableAdvertisement},
        Connection,
    },
    Softdevice,
};
use postcard::experimental::max_size::MaxSize;

use crate::messages::device_to_device::DeviceToDevice;

use super::ble::CENTRAL_ADDRESS;

#[nrf_softdevice::gatt_service(uuid = "cb6dcd5e-7f1b-11ef-8c55-a71ac707ac76")]
pub struct SplitService {
    #[characteristic(uuid = "2e72e4a2-7f1c-11ef-a1c2-13ed5ddd22d6", read, notify)]
    to_central: [u8; DeviceToDevice::POSTCARD_MAX_SIZE],

    #[characteristic(uuid = "3969f044-7f1c-11ef-96c9-7ba31210e355", write_without_response)]
    to_peripheral: [u8; DeviceToDevice::POSTCARD_MAX_SIZE],
}

#[nrf_softdevice::gatt_server]
pub struct SplitServer {
    service: SplitService,
}

async fn process_open_connection(
    server: &SplitServer,
    conn: &Connection,
    mut on_rx: impl FnMut(DeviceToDevice),
    mut get_tx: impl async FnMut() -> DeviceToDevice,
) {
    let server_fut = run(conn, server, |evt| match evt {
        SplitServerEvent::Service(evt_) => match evt_ {
            SplitServiceEvent::ToCentralCccdWrite { notifications: _ } => {}
            SplitServiceEvent::ToPeripheralWrite(msg) => {
                let Ok(deser) = postcard::from_bytes::<DeviceToDevice>(&msg) else {
                    return;
                };

                crate::log::trace!("Got message: {:?}", deser);

                on_rx(deser);
            }
        },
    });

    let sender_fut = async {
        loop {
            let msg = get_tx().await;
            let mut buf = [0u8; DeviceToDevice::POSTCARD_MAX_SIZE];
            postcard::to_slice(&msg, &mut buf).unwrap();

            crate::log::trace!("Sending message: {:?}", msg);

            if server.service.to_central_notify(conn, &buf).is_err() {
                break;
            };
        }
    };

    embassy_futures::select::select(server_fut, sender_fut).await;
}

pub async fn advertisement_loop(
    sd: &Softdevice,
    server: &SplitServer,
    mut on_rx: impl FnMut(DeviceToDevice),
    mut get_tx: impl async FnMut() -> DeviceToDevice,
) {
    loop {
        let adv = ConnectableAdvertisement::NonscannableDirected {
            peer: CENTRAL_ADDRESS,
        };

        crate::log::debug!("Beginning to advertise");

        let conn = match advertise_connectable(sd, adv, &Default::default()).await {
            Ok(conn) => conn,
            Err(err) => {
                crate::log::error!("Failed to advertise? {}", err);
                continue;
            }
        };

        crate::log::info!("Connected to other side: {}", conn.peer_address());

        process_open_connection(server, &conn, &mut on_rx, &mut get_tx).await;
    }
}
