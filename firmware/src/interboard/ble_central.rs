use embassy_time::Duration;
use postcard::experimental::max_size::MaxSize;
use trouble_host::prelude::*;

use crate::{
    ble::{BleStack, Controller, Pool},
    messages::device_to_device::DeviceToDevice,
};

use super::ble::{
    peripheral_address, INTERBOARD_SERVER_TO_CENTRAL, INTERBOARD_SERVER_TO_PERIPHERAL,
    INTERBOARD_SERVER_UUID,
};

type Message = [u8; DeviceToDevice::POSTCARD_MAX_SIZE];

async fn process_open_connection(
    stack: &BleStack,
    conn: &Connection<'_, Pool>,
    mut on_rx: impl FnMut(DeviceToDevice),
    mut get_tx: impl async FnMut() -> DeviceToDevice,
) -> Result<(), BleHostError<nrf_sdc::Error>> {
    let client = GattClient::<Controller, Pool, 4>::new(stack, conn).await?;

    let session = async {
        let service = client
            .services_by_uuid(&Uuid::from(INTERBOARD_SERVER_UUID))
            .await?
            .first()
            .cloned()
            .ok_or(Error::NotFound)?;

        let to_central: Characteristic<Message> = client
            .characteristic_by_uuid(&service, &Uuid::from(INTERBOARD_SERVER_TO_CENTRAL))
            .await?;
        let to_peripheral: Characteristic<Message> = client
            .characteristic_by_uuid(&service, &Uuid::from(INTERBOARD_SERVER_TO_PERIPHERAL))
            .await?;

        let mut listener = client.subscribe(&to_central, false).await?;

        crate::log::debug!("Connected to peripheral");

        let receiver = async {
            loop {
                let notification = listener.next().await;
                let Ok(deser) = postcard::from_bytes::<DeviceToDevice>(notification.as_ref())
                else {
                    continue;
                };

                on_rx(deser);
            }
        };

        let sender = async {
            loop {
                let msg = get_tx().await;
                let mut buf = [0u8; DeviceToDevice::POSTCARD_MAX_SIZE];
                postcard::to_slice(&msg, &mut buf).unwrap();

                if let Err(e) = client
                    .write_characteristic_without_response(&to_peripheral, &buf)
                    .await
                {
                    return e;
                }
            }
        };

        match embassy_futures::select::select(receiver, sender).await {
            embassy_futures::select::Either::First(()) => Ok(()),
            embassy_futures::select::Either::Second(e) => Err(e),
        }
    };

    // the client task has to run for any of the requests to complete
    match embassy_futures::select::select(client.task(), session).await {
        embassy_futures::select::Either::First(r) => r,
        embassy_futures::select::Either::Second(r) => r,
    }
}

pub async fn central_loop(
    stack: &'static BleStack,
    mut on_rx: impl FnMut(DeviceToDevice),
    mut get_tx: impl async FnMut() -> DeviceToDevice,
) {
    let mut central = stack.central();
    let filter = [peripheral_address()];

    loop {
        let config = ConnectConfig {
            scan_config: ScanConfig {
                filter_accept_list: &filter,
                ..Default::default()
            },
            connect_params: RequestedConnParams {
                min_connection_interval: Duration::from_micros(7_500),
                max_connection_interval: Duration::from_micros(7_500),
                max_latency: 30,
                supervision_timeout: Duration::from_secs(4),
                ..Default::default()
            },
        };

        crate::log::debug!("Scanning for peripheral");

        let conn = match central.connect(&config).await {
            Ok(conn) => conn,
            Err(err) => {
                crate::log::error!("Failed to connect?: {}", defmt::Debug2Format(&err));

                continue;
            }
        };

        if let Err(err) = process_open_connection(stack, &conn, &mut on_rx, &mut get_tx).await {
            crate::log::error!(
                "Peripheral connection failed: {}",
                defmt::Debug2Format(&err)
            );
        }

        crate::log::debug!("Peripheral disconnected");
    }
}
