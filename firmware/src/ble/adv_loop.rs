use super::{dfu::DfuConfig, server::GloveServer};
use crate::{
    ble::{bonder::Bonder, dfu::NrfDfuServiceEvent},
    interboard::{channel::COMMANDS_TO_OTHER_SIDE, THIS_SIDE_MESSAGE_BUS},
    state::with_advertising,
};
use embassy_boot::AlignedBuffer;
use embassy_sync::{blocking_mutex::raw::ThreadModeRawMutex, channel::Channel};
use nrf_dfu_target::prelude::{DfuStatus, FirmwareInfo, FirmwareType, HardwareInfo};
use nrf_softdevice::{
    ble::{
        advertisement_builder::{
            AdvertisementDataType, Flag, LegacyAdvertisementBuilder, LegacyAdvertisementPayload,
            ServiceUuid16,
        },
        gatt_server,
        peripheral::{self, advertise_connectable, ConnectableAdvertisement},
    },
    Softdevice,
};

pub async fn advertisement_loop(
    sd: &'static Softdevice,
    server: GloveServer,
    #[allow(unused)] bonder: &'static Bonder,
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

    let spawner = embassy_executor::Spawner::for_current_executor().await;

    loop {
        crate::log::info!("Advertising ourselves");

        let config = peripheral::Config::default();
        let adv = ConnectableAdvertisement::ScannableUndirected {
            adv_data: &ADV_DATA,
            scan_data: &SCAN_DATA,
        };

        let conn =
            // with_advertising(advertise_pairable(sd, adv, &config, temp_bonder)),
            with_advertising(advertise_connectable(sd, adv, &config)).await.unwrap();

        // bonder.load_sys_attrs(&conn);

        crate::log::info!(
            "Device connected: {} ({})",
            conn.peer_address(),
            conn.security_mode()
        );

        // unsure if this is needed?
        // if let Err(e) = conn.request_security() {
        //     crate::log::info!("Failed to auth connection: {}", e);
        //     _ = conn.disconnect_with_reason(HciStatus::AUTHENTICATION_FAILURE);
        //     continue;
        // }

        let _ = spawner.spawn(handle_connection(
            conn,
            part,
            variant,
            dfuconfig.clone(),
            server.clone(),
        ));
    }
}

#[embassy_executor::task(pool_size = 3)]
async fn handle_connection(
    conn: nrf_softdevice::ble::Connection,
    part: u32,
    variant: u32,
    dfuconfig: DfuConfig,
    server: GloveServer,
) {
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

    let hid_processor = async {
        if let Some(hid) = server.hid.as_ref() {
            hid.send_reports(&conn).await;
        } else {
            // if there's no hid server this one should run forever
            core::future::pending::<()>().await;
        }
    };

    let split_processor = async {
        if let Some(split) = server.split.as_ref() {
            let rx_fn = || async { COMMANDS_TO_OTHER_SIDE.receive().await.msg };

            split.transmit_loop(&conn, rx_fn).await;
        } else {
            // if there's no hid server this one should run forever
            core::future::pending::<()>().await;
        }
    };

    let dfu_command_processor = async {
        loop {
            let evt = msg_chan.receive().await;

            crate::log::debug!("Handling DFU command");

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

                        embassy_time::Timer::after_millis(300).await;

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

    let msg_pub = THIS_SIDE_MESSAGE_BUS.publisher().unwrap();

    let gatt = gatt_server::run(&conn, &server, |e| match e {
        crate::ble::server::GloveServerEvent::DFU(e) => {
            if let Err(_) = msg_chan.try_send(e) {
                crate::log::error!(
                    "Missed a DFU packet while transferring it to async land... oops"
                );
            }
        }
        crate::ble::server::GloveServerEvent::HID(_) => {
            // there's nothing to do here
        }
        crate::ble::server::GloveServerEvent::Split(evt) => {
            if let Some(split) = server.split.as_ref() {
                split.process(evt, |e| {
                    let _ = msg_pub.try_publish(e);
                });
            }
        }
    });

    match embassy_futures::select::select4(
        hid_processor,
        split_processor,
        dfu_command_processor,
        gatt,
    )
    .await
    {
        embassy_futures::select::Either4::First(_) => crate::log::debug!("Hid quit"),
        embassy_futures::select::Either4::Second(_) => crate::log::debug!("Split quit"),
        embassy_futures::select::Either4::Third(_) => crate::log::debug!("dfu quit"),
        embassy_futures::select::Either4::Fourth(_) => crate::log::debug!("gatt server quit"),
    }

    crate::log::debug!("Device disconnected");
}
