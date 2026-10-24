//! One network stack with two interfaces: the keyboard's 802.15.4 network,
//! and the USB ethernet link to the host
//!
//! The stack doesn't forward between interfaces, so the host only reaches the
//! halves through the ports [`crate::proxy`] forwards.

use embassy_executor::{InterruptExecutor, Spawner};
use embassy_net::{
    iface::{dhcpv4_server::DhcpServerConfig, Iface},
    wire::{IpCidr, Ipv4Cidr},
    Stack, StackStorage, StaticPool,
};
use embassy_nrf::{
    interrupt,
    interrupt::{InterruptExt, Priority},
    radio::ieee802154::Radio,
};
use glove_net::{pool::CountingPool, radio};
use shared::net::{usb as usb_net, Node};

use crate::{singleton, usb::EthernetDevice};

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
        defmt::info!("radio: {}", radio::stats());
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
    glove_net::keep_neighbors(stack, &iface, Node::Dongle).await
}

// fragmentation, reassembly and the queues of both drivers all hold buffers
static POOL: CountingPool<StaticPool<1514, 32>> = CountingPool::new(StaticPool::new());

/// How many times the network stack wanted a packet buffer and had none
pub fn pool_exhausted() -> u32 {
    POOL.exhausted()
}

pub fn init(
    spawner: &Spawner,
    radio: Radio<'static>,
    ethernet: EthernetDevice,
    seed: u64,
) -> Stack<'static> {
    let (device, bridge, mac) = radio::new(
        Node::Dongle.mac(),
        radio,
        (seed >> 32) as u32,
        singleton!(radio::State<4, 4>, radio::State::new()),
    );

    interrupt::EGU0_SWI0.set_priority(Priority::P1);
    let radio_spawner = RADIO_EXECUTOR.start(interrupt::EGU0_SWI0);
    radio_spawner.spawn(mac_task(mac).unwrap());
    spawner.spawn(bridge_task(bridge).unwrap());
    spawner.spawn(stats_task().unwrap());

    let (stack, runner) = Stack::new(
        singleton!(StackStorage<'static>, StackStorage::new()),
        &POOL,
        seed,
    );

    let radio_iface = stack
        .add_iface_borrowed(singleton!(radio::Device<'static>, device))
        .unwrap();
    glove_net::configure_radio_iface(&radio_iface, Node::Dongle);

    let usb_iface = stack
        .add_iface_borrowed(singleton!(EthernetDevice, ethernet))
        .unwrap();
    // link-local addresses must only mean the radio, that's how we reach
    // the halves
    for addr in usb_iface.ip_addrs() {
        if let IpCidr::V6(cidr) = addr.cidr {
            if cidr.address().is_unicast_link_local() {
                usb_iface.remove_ip_addr(cidr.address());
            }
        }
    }
    usb_iface
        .add_ip_addr(IpCidr::V4(Ipv4Cidr::new(
            usb_net::DONGLE_IP,
            usb_net::PREFIX_LEN,
        )))
        .unwrap();
    // no gateway, the host shouldn't send anything but our traffic this way
    usb_iface
        .set_dhcpv4_server(Some(DhcpServerConfig::new(
            usb_net::POOL_START,
            usb_net::POOL_END,
        )))
        .unwrap();

    spawner.spawn(net_task(runner).unwrap());
    spawner.spawn(neighbors_task(stack, radio_iface).unwrap());

    stack
}
