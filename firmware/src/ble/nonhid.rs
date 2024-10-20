use super::{dfu::DfuConfig, server::NonHIDServer};
use crate::{
    ble::{bonder::Bonder, dfu::NrfDfuServiceEvent},
    state::{wait_usb_disconnected, with_advertising},
};
use embassy_boot::AlignedBuffer;
use embassy_sync::{blocking_mutex::raw::ThreadModeRawMutex, channel::Channel};
use nrf_dfu_target::prelude::{DfuStatus, FirmwareInfo, FirmwareType, HardwareInfo};
use nrf_softdevice::{
    ble::{
        advertisement_builder::{
            AdvertisementDataType, Flag, LegacyAdvertisementBuilder, LegacyAdvertisementPayload, ServiceUuid16
        },
        gatt_server,
        peripheral::{self, advertise_connectable, ConnectableAdvertisement},
        HciStatus,
    },
    Softdevice,
};

pub async fn advertisement_loop_nonhid(
    sd: &Softdevice,
    server: &NonHIDServer,
    #[allow(unused)]
    bonder: &'static Bonder,
    dfuconfig: DfuConfig,
) {
    static ADV_DATA: LegacyAdvertisementPayload = LegacyAdvertisementBuilder::new()
        .flags(&[Flag::GeneralDiscovery, Flag::LE_Only])
        .services_16(
            nrf_softdevice::ble::advertisement_builder::ServiceList::Incomplete,
            &[
                ServiceUuid16::DEVICE_INFORMATION, // TODO: battery
                ServiceUuid16::from_u16(0xFE59),
            ],
        )
        .raw(AdvertisementDataType::APPEARANCE, &[0xC1, 0x03])
        .full_name("Glove80 LH")
        .build();

    static SCAN_DATA: LegacyAdvertisementPayload = LegacyAdvertisementBuilder::new()
        .services_16(
            nrf_softdevice::ble::advertisement_builder::ServiceList::Incomplete,
            &[
                // TODO: battery
                ServiceUuid16::DEVICE_INFORMATION,
                ServiceUuid16::from_u16(0xFE59),
            ],
        )
        .build();

    let p = unsafe { embassy_nrf::pac::Peripherals::steal() };
    let part = p.FICR.info.part.read().part().bits();
    let variant = p.FICR.info.variant.read().variant().bits();

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
            // with_advertising(advertise_pairable(sd, adv, &config, temp_bonder)),
            with_advertising(advertise_connectable(sd, adv, &config)),
            wait_usb_disconnected(),
        )
        .await
        {
            embassy_futures::select::Either::First(conn) => conn.unwrap(),
            embassy_futures::select::Either::Second(()) => continue,
        };

        // bonder.load_sys_attrs(&conn);

        crate::log::info!(
            "Device connected: {} ({})",
            conn.peer_address(),
            conn.security_mode()
        );

        // unsure if this is needed?
        if let Err(e) = conn.request_security() {
            crate::log::info!("Failed to auth connection: {}", e);
            _ = conn.disconnect_with_reason(HciStatus::AUTHENTICATION_FAILURE);
            continue;
        }

        let mut conn_handle = super::dfu::ConnectionHandle {
            connection: conn.clone(),
            notify_control: false,
            notify_packet: false,
        };

        let hw_info = HardwareInfo {
            part,
            variant,
            rom_size: 0,
            ram_size: 0,
            rom_page_size: 0,
        };

        let fw_info = FirmwareInfo {
            ftype: FirmwareType::Application,
            version: 1,
            addr: 0,
            len: 0,
        };

        let mut dfu = dfuconfig.dfu();
        let mut target = super::dfu::Target::new(dfu.size(), fw_info, hw_info);

        let msg_chan = Channel::<ThreadModeRawMutex, NrfDfuServiceEvent, 16>::new();

        let command_processor = async {
            loop {
                let evt = msg_chan.receive().await;

                crate::log::debug!("Handling DFU command");

                // TODO: Updater seems to lock up, but I think that's because
                // nrf_dfu_target is not replying properly

                let state = server
                    .dfu
                    .handle(&mut target, &mut dfu, &mut conn_handle, evt)
                    .await;
                crate::log::debug!("New dfu status: {}", defmt::Debug2Format(&state));

                if let Some(DfuStatus::DoneReset) = state {
                    let mut magic = AlignedBuffer([0; 4]);
                    let mut state =
                        embassy_boot_nrf::FirmwareState::new(dfuconfig.state(), &mut magic.0);

                    match state.mark_updated().await {
                        Ok(_) => {
                            crate::log::info!("Going down for update");
                            cortex_m::peripheral::SCB::sys_reset();
                        }
                        Err(e) => {
                            panic!("Error while updating: {:?}", e);
                        }
                    }
                }

                crate::log::debug!("Done handling DFU command");
            }
        };

        let gatt = gatt_server::run(&conn, server, |e| match e {
            crate::ble::server::NonHIDEvent::DFU(e) => {
                if let Err(_) = msg_chan.try_send(e) {
                    crate::log::error!(
                        "Missed a DFU packet while transferring it to async land... oops"
                    );
                }
            }
        });

        embassy_futures::select::select(command_processor, gatt).await;

        crate::log::debug!("Device disconnected")
    }
}
