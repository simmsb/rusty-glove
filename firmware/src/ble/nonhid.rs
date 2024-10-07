use core::cell::OnceCell;

use crate::{
    ble::bonder::{load_bonder, Bonder},
    singleton,
    state::{wait_usb_disconnected, with_advertising},
};

use super::server::NonHIDServer;
use nrf_softdevice::{
    ble::{
        advertisement_builder::{
            Flag, LegacyAdvertisementBuilder, LegacyAdvertisementPayload, ServiceUuid16,
        },
        gatt_server,
        peripheral::{self, advertise_pairable, ConnectableAdvertisement},
        HciStatus,
    },
    Softdevice,
};

pub async fn advertisement_loop_nonhid(
    sd: &Softdevice,
    server: &NonHIDServer,
    bonder: &'static Bonder,
) {
    static ADV_DATA: LegacyAdvertisementPayload = LegacyAdvertisementBuilder::new()
        .flags(&[Flag::GeneralDiscovery, Flag::LE_Only])
        .services_16(
            nrf_softdevice::ble::advertisement_builder::ServiceList::Incomplete,
            &[
                ServiceUuid16::DEVICE_INFORMATION, // TODO: battery
            ],
        )
        .full_name("Glove80 LH")
        .build();

    static SCAN_DATA: LegacyAdvertisementPayload = LegacyAdvertisementBuilder::new()
        .services_16(
            nrf_softdevice::ble::advertisement_builder::ServiceList::Complete,
            &[
                // TODO: battery
                ServiceUuid16::DEVICE_INFORMATION,
            ],
        )
        .build();

    loop {
        crate::log::info!("Waiting for usb to connect");
        crate::state::wait_usb_connected().await;

        crate::log::info!("Advertising as the non hid device");

        let config = peripheral::Config::default();
        let adv = ConnectableAdvertisement::ScannableUndirected {
            adv_data: &ADV_DATA,
            scan_data: &SCAN_DATA,
        };

        let conn = match embassy_futures::select::select(
            with_advertising(advertise_pairable(sd, adv, &config, bonder)),
            wait_usb_disconnected(),
        )
        .await
        {
            embassy_futures::select::Either::First(conn) => conn.unwrap(),
            embassy_futures::select::Either::Second(()) => continue,
        };

        crate::log::info!("Device connected");

        if let Err(e) = conn.request_security() {
            crate::log::info!("Failed to auth connection: {}", e);
            _ = conn.disconnect_with_reason(HciStatus::AUTHENTICATION_FAILURE);
            continue;
        }

        gatt_server::run(&conn, server, |_| {
            // TODO: handle writes for fwupd here
        })
        .await;
    }
}
