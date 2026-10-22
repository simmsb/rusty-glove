use super::{bonder, dfu::DfuConfig, server::GloveServer, BleStack, Pool};
use crate::{
    ble::dfu::{NrfDfuServiceEvent, ATT_MTU},
    state::with_advertising,
};
use embassy_boot::AlignedBuffer;
use embassy_sync::{blocking_mutex::raw::ThreadModeRawMutex, channel::Channel};
use embassy_time::{Duration, Timer};
use futures::never::Never;
use heapless::Vec;
use nrf_dfu_target::prelude::{DfuStatus, FirmwareInfo, FirmwareType, HardwareInfo};
use trouble_host::prelude::*;

pub async fn advertisement_loop(
    stack: &'static BleStack,
    server: &'static GloveServer<'static>,
    dfuconfig: DfuConfig,
) {
    let services: &[[u8; 2]] = if crate::side::is_master() {
        &[
            service::DEVICE_INFORMATION.to_le_bytes(),
            service::HUMAN_INTERFACE_DEVICE.to_le_bytes(),
        ]
    } else {
        &[service::DEVICE_INFORMATION.to_le_bytes()]
    };

    let name = if crate::side::is_master() {
        "Glove80 RH"
    } else {
        "Glove80 LH"
    };

    let mut adv_data = [0u8; 31];
    let adv_len = AdStructure::encode_slice(
        &[
            AdStructure::Flags(LE_GENERAL_DISCOVERABLE | BR_EDR_NOT_SUPPORTED),
            AdStructure::IncompleteServiceUuids16(services),
            // appearance: keyboard
            AdStructure::Unknown {
                ty: 0x19,
                data: &[0xC1, 0x03],
            },
            AdStructure::CompleteLocalName(name.as_bytes()),
        ],
        &mut adv_data[..],
    )
    .unwrap();

    let mut scan_data = [0u8; 31];
    let scan_len = AdStructure::encode_slice(
        &[AdStructure::IncompleteServiceUuids16(services)],
        &mut scan_data[..],
    )
    .unwrap();

    let part = embassy_nrf::pac::FICR.info().part().read().part().to_bits();
    let variant = embassy_nrf::pac::FICR.info().variant().read();

    // SAFETY: we're running inside an embassy task
    let spawner = unsafe { embassy_executor::Spawner::for_current_executor() }.await;

    let mut peripheral = stack.peripheral();

    loop {
        crate::log::info!("Advertising ourselves");

        let conn = with_advertising(async {
            let advertiser = peripheral
                .advertise(
                    &Default::default(),
                    Advertisement::ConnectableScannableUndirected {
                        adv_data: &adv_data[..adv_len],
                        scan_data: &scan_data[..scan_len],
                    },
                )
                .await?;
            Ok::<_, BleHostError<nrf_sdc::Error>>(advertiser.accept().await?)
        })
        .await;

        let conn = match conn.map(|c| c.with_attribute_server(server)) {
            Ok(Ok(conn)) => conn,
            Ok(Err(e)) => {
                crate::log::error!("Failed to attach gatt server: {}", defmt::Debug2Format(&e));
                continue;
            }
            Err(e) => {
                crate::log::error!("Failed to advertise: {}", defmt::Debug2Format(&e));
                Timer::after_secs(1).await;
                continue;
            }
        };

        _ = conn.raw().set_bondable(true);

        crate::log::info!("Device connected: {}", conn.raw().peer_address());

        match handle_connection(conn, stack, server, part, variant, dfuconfig.clone()) {
            Ok(token) => spawner.spawn(token),
            Err(_) => crate::log::warn!("Too many connections, dropping this one"),
        }
    }
}

#[embassy_executor::task(pool_size = 3)]
async fn handle_connection(
    conn: GattConnection<'static, 'static, Pool>,
    stack: &'static BleStack,
    server: &'static GloveServer<'static>,
    part: u32,
    variant: u32,
    dfuconfig: DfuConfig,
) {
    bonder::load_att_table(server, conn.raw());

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

    let msg_chan = Channel::<ThreadModeRawMutex, NrfDfuServiceEvent, 4>::new();

    // set when the connection quits, used to ask the dfu processor to quit as
    // it might have some more work to do.
    let quitting = crate::sync::WaitCell::new();

    // set when the peer subscribes to messages from this side, only the master
    // does this, so we don't steal messages meant for it on host connections
    #[cfg(not(feature = "side_right"))]
    let split_subscribed = crate::sync::Watch::new(false);

    let set_connparam = async {
        Timer::after_secs(5).await;

        let params = conn.raw().params();
        crate::log::info!(
            "Current params. interval: {}us, lat: {}, timeout: {}ms",
            params.conn_interval.as_micros(),
            params.peripheral_latency,
            params.supervision_timeout.as_millis()
        );

        if params.conn_interval > Duration::from_micros(15_000) {
            if let Err(e) = conn
                .raw()
                .update_connection_params(
                    stack,
                    &RequestedConnParams {
                        min_connection_interval: Duration::from_micros(15_000),
                        max_connection_interval: Duration::from_micros(15_000),
                        max_latency: 99,
                        supervision_timeout: Duration::from_secs(5),
                        ..Default::default()
                    },
                )
                .await
            {
                crate::log::warn!("Failed to update conn params: {}", defmt::Debug2Format(&e));
            }

            Timer::after_millis(50).await;

            if let Err(e) = conn
                .raw()
                .update_connection_params(
                    stack,
                    &RequestedConnParams {
                        min_connection_interval: Duration::from_micros(7_500),
                        max_connection_interval: Duration::from_micros(7_500),
                        max_latency: 30,
                        supervision_timeout: Duration::from_secs(5),
                        ..Default::default()
                    },
                )
                .await
            {
                crate::log::warn!("Failed to update conn params: {}", defmt::Debug2Format(&e));
            }
        }

        core::future::pending::<Never>().await
    };

    #[cfg(feature = "side_right")]
    let role_processor = server.hid.send_reports(&conn);

    #[cfg(not(feature = "side_right"))]
    let role_processor = async {
        use crate::interboard::channel::COMMANDS_TO_OTHER_SIDE;

        split_subscribed.wait_for(|s| *s).await;

        let rx_fn = || async { COMMANDS_TO_OTHER_SIDE.receive().await.msg };

        server.split.transmit_loop(&conn, rx_fn).await
    };

    let dfu_command_processor = async {
        loop {
            let evt =
                match embassy_futures::select::select(msg_chan.receive(), quitting.wait()).await {
                    embassy_futures::select::Either::First(evt) => evt,
                    embassy_futures::select::Either::Second(_msg) => {
                        crate::log::debug!("DFU processor told to quit");

                        return;
                    }
                };

            crate::log::debug!("Handling DFU command");

            let state = server.dfu.handle(&mut target, &mut dfu, &conn, evt).await;
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
                        defmt::panic!("Error while updating: {:?}", defmt::Debug2Format(&e));
                    }
                }
            }

            crate::log::debug!("Done handling DFU command");
        }
    };

    #[cfg(not(feature = "side_right"))]
    let msg_pub = crate::interboard::THIS_SIDE_MESSAGE_BUS
        .publisher()
        .unwrap();

    let on_write = async |handle: u16, data: &[u8]| {
        if let Some(e) = server.dfu.on_write(handle, data) {
            msg_chan.send(e).await;
        }

        #[cfg(not(feature = "side_right"))]
        {
            if handle == server.split.to_peripheral.handle {
                server.split.process(data, |e| {
                    let _ = msg_pub.try_publish(e);
                });
            }

            if Some(handle) == server.split.to_central.cccd_handle {
                split_subscribed.set(data.first().is_some_and(|b| b & 1 != 0));
            }
        }
    };

    let gatt = async {
        let reason = loop {
            match conn.next().await {
                GattConnectionEvent::Disconnected { reason } => break reason,
                GattConnectionEvent::PairingComplete {
                    security_level,
                    bond,
                } => {
                    crate::log::info!("Pairing complete: {}", defmt::Debug2Format(&security_level));

                    // the peer's identity is known now, so pick up its saved
                    // subscriptions before (re)recording the bond
                    bonder::load_att_table(server, conn.raw());

                    if let Some(bond) = bond {
                        bonder::on_bonded(stack, server, conn.raw(), bond);
                    }
                }
                GattConnectionEvent::Encrypted { security_level, .. } => {
                    crate::log::info!("Encrypted: {}", defmt::Debug2Format(&security_level));

                    // a bonded peer reconnecting, its identity is resolved by now
                    bonder::load_att_table(server, conn.raw());
                }
                GattConnectionEvent::PairingFailed(e) => {
                    crate::log::warn!("Pairing failed: {}", defmt::Debug2Format(&e));
                }
                GattConnectionEvent::Gatt { event } => {
                    // grab the data out before replying, the reply consumes the event
                    let write = match &event {
                        GattEvent::Write(w) => Some((
                            w.handle(),
                            w.with_data(|_offset, data| Vec::<u8, ATT_MTU>::from_slice(data).ok()),
                        )),
                        _ => None,
                    };

                    // reply first so the peer isn't kept waiting while we
                    // (potentially) write to flash
                    match event.accept() {
                        Ok(reply) => reply.send().await,
                        Err(e) => crate::log::warn!(
                            "Error sending gatt response: {}",
                            defmt::Debug2Format(&e)
                        ),
                    }

                    if let Some((handle, Some(data))) = write {
                        on_write(handle, &data).await;
                    }
                }
                _ => {}
            }
        };

        bonder::save_att_table(server, conn.raw());

        crate::log::debug!("Device disconnected: {}", defmt::Debug2Format(&reason));
    };

    let update_ts = async {
        loop {
            let now = embassy_time::Instant::now();
            _ = server
                .uptime
                .uptime
                .notify(&conn, &now.as_secs(), true)
                .await;

            Timer::after_secs(10).await;
        }
    };

    embassy_futures::join::join(dfu_command_processor, async {
        embassy_futures::select::select3(
            role_processor,
            gatt,
            embassy_futures::select::select(update_ts, set_connparam),
        )
        .await;
        quitting.wake();
    })
    .await;

    crate::log::debug!("Connection handler finished");
}
