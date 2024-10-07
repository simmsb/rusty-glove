use bonder::{load_bonder, save_bondinfo_loop};
use embassy_executor::Spawner;
use nrf_softdevice::Softdevice;
use server::NonHIDServer;

mod bonder;
mod device_info;
mod nonhid;
mod server;

pub fn make_nonhid_server(sd: &mut Softdevice) -> NonHIDServer {
    NonHIDServer::new(sd)
}

pub fn init_peripheral(spawner: &Spawner, sd: &'static Softdevice, server: NonHIDServer) {
    spawner.must_spawn(peripheral(sd, server));
}

#[embassy_executor::task]
async fn peripheral(sd: &'static Softdevice, server: NonHIDServer) {
    crate::log::debug!("Starting peripheral (to host) bt task");

    let bonder = load_bonder().await;

    embassy_executor::Spawner::for_current_executor()
        .await
        .must_spawn(save_bondinfo_loop(bonder));

    nonhid::advertisement_loop_nonhid(sd, &server, bonder).await;
}
