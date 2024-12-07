use bonder::{load_bonder, save_bondinfo_loop};
use dfu::DfuConfig;
use embassy_executor::Spawner;
use nrf_softdevice::Softdevice;
use server::GloveServer;

mod adv_loop;
mod bonder;
mod device_info;
pub mod dfu;
pub mod hid;
mod interboard;
mod server;

pub fn make_ble_server(sd: &mut Softdevice) -> GloveServer {
    GloveServer::new(sd)
}

pub fn init_peripheral(
    spawner: &Spawner,
    sd: &'static Softdevice,
    server: GloveServer,
    dfuconfig: DfuConfig,
) {
    crate::log::trace!("Setting up ble (peripheral)");
    spawner.must_spawn(peripheral(sd, server, dfuconfig));
}

#[embassy_executor::task]
async fn peripheral(sd: &'static Softdevice, server: GloveServer, dfuconfig: DfuConfig) {
    crate::log::debug!("Starting peripheral (to host) bt task");

    let bonder = load_bonder().await;

    embassy_executor::Spawner::for_current_executor()
        .await
        .must_spawn(save_bondinfo_loop(bonder));

    adv_loop::advertisement_loop(sd, server, bonder, dfuconfig).await;
}
