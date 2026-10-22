use dfu::DfuConfig;
use embassy_executor::Spawner;
use embassy_nrf::mode::Async;
use nrf_sdc::mpsl::MultiprotocolServiceLayer;
use server::GloveServer;
use trouble_host::prelude::*;

use crate::{rng::SdcRng, utils::singleton};

mod adv_loop;
mod bonder;
mod device_info;
pub mod dfu;
pub mod hid;
#[cfg(not(feature = "side_right"))]
mod interboard;
mod server;

pub type Controller = nrf_sdc::SoftdeviceController<'static>;
pub type Pool = DefaultPacketPool;
pub type BleStack = Stack<'static, Controller, Pool>;

/// Connections to hosts, plus the link between the two halves
pub const CONNECTIONS_MAX: usize = 4;
const PERIPHERAL_CONNECTIONS_MAX: u8 = 3;
const L2CAP_CHANNELS_MAX: usize = CONNECTIONS_MAX * 2;

/// Size of the controller's l2cap buffers, matches the default packet pool
const L2CAP_MTU: u16 = 251;
const L2CAP_TXQ: u8 = 3;
const L2CAP_RXQ: u8 = 3;

// generous, the controller logs how much it actually needed at startup
const SDC_MEM_SIZE: usize = 32 * 1024;

pub fn make_controller(
    p: nrf_sdc::Peripherals<'static>,
    rng: &'static mut embassy_nrf::rng::Rng<'static, Async>,
    mpsl: &'static MultiprotocolServiceLayer<'static>,
) -> Controller {
    let rng = singleton!(SdcRng, SdcRng(rng));
    let mem = singleton!(nrf_sdc::Mem<SDC_MEM_SIZE>, nrf_sdc::Mem::new());

    nrf_sdc::Builder::new()
        .and_then(|b| {
            b.support_adv()
                .support_peripheral()
                .support_scan()
                .support_central()
                .support_dle_peripheral()
                .support_dle_central()
                .peripheral_count(PERIPHERAL_CONNECTIONS_MAX)
        })
        .and_then(|b| b.central_count(1))
        .and_then(|b| b.buffer_cfg(L2CAP_MTU, L2CAP_MTU, L2CAP_TXQ, L2CAP_RXQ))
        .and_then(|b| b.build(p, rng, mpsl, mem))
        .unwrap()
}

/// The master side uses the factory programmed address (same as the
/// softdevice did), the other side uses a fixed address so the master can find
/// it.
fn our_address() -> Address {
    if crate::side::is_master() {
        let ficr = embassy_nrf::pac::FICR;
        let lo = ficr.deviceaddr(0).read().to_le_bytes();
        let hi = ficr.deviceaddr(1).read().to_le_bytes();
        let mut addr = [lo[0], lo[1], lo[2], lo[3], hi[0], hi[1]];
        // random static addresses have the top two bits set
        addr[5] |= 0xc0;
        Address::random(addr)
    } else {
        crate::interboard::ble::peripheral_address()
    }
}

pub async fn init(spawner: &Spawner, controller: Controller, dfuconfig: DfuConfig) {
    crate::log::trace!("Setting up ble");

    let resources = singleton!(
        HostResources<Pool, CONNECTIONS_MAX, L2CAP_CHANNELS_MAX>,
        HostResources::new()
    );
    let stack = singleton!(
        BleStack,
        trouble_host::new(controller, resources)
            .set_random_address(our_address())
            .set_io_capabilities(IoCapabilities::NoInputNoOutput)
            .build()
    );

    bonder::load_bonds(stack).await;

    let server = singleton!(GloveServer<'static>, server::make_server());

    spawner.spawn(ble_runner(stack).unwrap());
    spawner.spawn(bonder::save_bondinfo_loop().unwrap());
    spawner.spawn(peripheral(stack, server, dfuconfig).unwrap());

    if crate::side::is_master() {
        crate::interboard::init_central(spawner, stack);
    }
}

#[embassy_executor::task]
async fn ble_runner(stack: &'static BleStack) {
    let mut runner = stack.runner();

    loop {
        if let Err(e) = runner.run().await {
            crate::log::error!("BLE runner exited: {}", defmt::Debug2Format(&e));
        }
    }
}

#[embassy_executor::task]
async fn peripheral(
    stack: &'static BleStack,
    server: &'static GloveServer<'static>,
    dfuconfig: DfuConfig,
) {
    crate::log::debug!("Starting peripheral (to host) bt task");

    adv_loop::advertisement_loop(stack, server, dfuconfig).await;
}
