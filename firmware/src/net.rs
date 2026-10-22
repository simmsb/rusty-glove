//! Our end of the 802.15.4 network, see [`shared::net`]

use embassy_executor::{InterruptExecutor, Spawner};
use embassy_net::{iface::Iface, Stack, StackStorage, StaticPool};
use embassy_nrf::{
    interrupt,
    interrupt::{InterruptExt, Priority},
    radio::ieee802154::Radio,
};
use glove_net::radio;
use shared::{net::Node, side::KeyboardSide};

use crate::utils::singleton;

pub const fn this_node() -> Node {
    match crate::side::get_side() {
        KeyboardSide::Left => Node::Left,
        KeyboardSide::Right => Node::Right,
    }
}

/// Runs the radio above everything else, so acks go out in time
static RADIO_EXECUTOR: InterruptExecutor = InterruptExecutor::new();

#[interrupt]
unsafe fn EGU0_SWI0() {
    unsafe { RADIO_EXECUTOR.on_interrupt() }
}

#[embassy_executor::task]
async fn mac_task(mac: radio::Mac<'static>) -> ! {
    mac.run().await
}

/// How the radio is doing, cheap enough to leave running
#[embassy_executor::task]
async fn stats_task() -> ! {
    loop {
        embassy_time::Timer::after_secs(10).await;
        crate::log::info!("radio: {}", radio::stats());
    }
}

#[embassy_executor::task]
async fn bridge_task(bridge: radio::Bridge<'static>) -> ! {
    bridge.run().await
}

#[embassy_executor::task]
async fn net_task(mut runner: embassy_net::Runner<'static>) -> ! {
    runner.run().await
}

#[embassy_executor::task]
async fn neighbors_task(stack: Stack<'static>, iface: Iface<'static>) -> ! {
    glove_net::keep_neighbors(stack, &iface, this_node()).await
}

pub fn init(spawner: &Spawner, radio: Radio<'static>, seed: u64) -> Stack<'static> {
    crate::log::info!("Initialising network as {}", this_node());

    let (device, bridge, mac) = radio::new(
        this_node().mac(),
        radio,
        (seed >> 32) as u32,
        singleton!(radio::State<4, 4>, radio::State::new()),
    );

    interrupt::EGU0_SWI0.set_priority(Priority::P1);
    let radio_spawner = RADIO_EXECUTOR.start(interrupt::EGU0_SWI0);
    radio_spawner.spawn(mac_task(mac).unwrap());
    spawner.spawn(bridge_task(bridge).unwrap());
    spawner.spawn(stats_task().unwrap());

    // a fragmented packet waiting to go out, one being reassembled, and both
    // directions of the radio driver all hold buffers
    static POOL: StaticPool<1514, 24> = StaticPool::new();
    let (stack, runner) = Stack::new(
        singleton!(StackStorage<'static>, StackStorage::new()),
        &POOL,
        seed,
    );

    let iface = stack
        .add_iface_borrowed(singleton!(radio::Device<'static>, device))
        .unwrap();
    glove_net::configure_radio_iface(&iface, this_node());

    spawner.spawn(net_task(runner).unwrap());
    spawner.spawn(neighbors_task(stack, iface).unwrap());

    stack
}
