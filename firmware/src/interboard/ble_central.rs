use embassy_time::Timer;
use nrf_softdevice::{
    ble::{
        central::{connect, ConnectConfig},
        gatt_client::{self, discover},
        Connection,
    },
    Softdevice,
};
use postcard::experimental::max_size::MaxSize;

use crate::messages::device_to_device::DeviceToDevice;

use super::ble::PERIPHERAL_ADDRESS;

#[nrf_softdevice::gatt_client(uuid = "cb6dcd5e-7f1b-11ef-8c55-a71ac707ac76")]
struct SplitClient {
    #[characteristic(uuid = "2e72e4a2-7f1c-11ef-a1c2-13ed5ddd22d6", read, notify)]
    to_central: [u8; DeviceToDevice::POSTCARD_MAX_SIZE],

    #[characteristic(uuid = "3969f044-7f1c-11ef-96c9-7ba31210e355", write)]
    to_peripheral: [u8; DeviceToDevice::POSTCARD_MAX_SIZE],
}

async fn process_open_connection(
    client: SplitClient,
    conn: &Connection,
    mut on_rx: impl FnMut(DeviceToDevice),
    mut get_tx: impl async FnMut() -> DeviceToDevice,
) {
    client.to_central_cccd_write(true).await.unwrap();

    let client_fut = async {
        let reason = gatt_client::run(conn, &client, |evt| match evt {
            SplitClientEvent::ToCentralNotification(buf) => {
                let Ok(deser) = postcard::from_bytes::<DeviceToDevice>(&buf) else {
                    return;
                };

                on_rx(deser);
            }
        })
        .await;

        crate::log::debug!("Peripheral disconnected with reason: {}", reason);
    };

    let sender_fut = async {
        'outer: loop {
            let msg = get_tx().await;
            let mut buf = [0u8; DeviceToDevice::POSTCARD_MAX_SIZE];
            postcard::to_slice(&msg, &mut buf).unwrap();

            for n in 0u8..20 {
                if client
                    .to_peripheral_write_without_response(&buf)
                    .await
                    .is_ok()
                {
                    continue 'outer;
                };

                crate::log::trace!("Failed to send, backing off");

                Timer::after_micros(100 + n as u64 * 500).await;
            }

            break 'outer;
        }
    };

    embassy_futures::select::select(client_fut, sender_fut).await;
}

pub async fn central_loop(
    sd: &Softdevice,
    mut on_rx: impl FnMut(DeviceToDevice),
    mut get_tx: impl async FnMut() -> DeviceToDevice,
) {
    loop {
        let whitelist = [&PERIPHERAL_ADDRESS];

        let mut config = ConnectConfig::default();
        config.scan_config.whitelist = Some(&whitelist);
        config.conn_params.min_conn_interval = 6;
        config.conn_params.slave_latency = 30;
        config.conn_params.max_conn_interval = 6;

        crate::log::debug!("Scanning for peripheral");

        let conn = match connect(sd, &config).await {
            Ok(conn) => conn,
            Err(err) => {
                crate::log::error!("Failed to connect?: {}", err);

                continue;
            }
        };

        let client: SplitClient = match discover(&conn).await {
            Ok(client) => client,
            Err(err) => {
                crate::log::error!("Failed to discover?: {}", err);

                continue;
            }
        };

        crate::log::debug!("Connected to peripheral");

        process_open_connection(client, &conn, &mut on_rx, &mut get_tx).await;
    }
}
