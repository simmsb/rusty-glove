#![no_std]

//! The 802.15.4 network shared by the halves and the dongle

use embassy_net::{
    iface::Iface,
    wire::{
        HardwareAddress, Ieee802154Address, Ieee802154Pan, IpAddr, IpCidr, Ipv6Cidr,
        SixlowpanAddressContext,
    },
    Stack,
};
use embassy_time::{Duration, Instant, Timer};
use shared::net::{Node, PAN_ID, PREFIX, PREFIX_LEN};

pub mod dfu;
pub mod link;
pub mod radio;

/// How long a link can go without hearing from the other end
pub const LINK_TIMEOUT: Duration = Duration::from_secs(3);
/// How often a link sends something, if it has had nothing else to send
pub const HEARTBEAT: Duration = Duration::from_secs(1);

/// How long a fragmented packet may take to arrive in full. Its fragments
/// come within milliseconds of each other, and while one is incomplete it
/// holds a reassembly slot, so this must be short or one lost fragment stops
/// fragmented packets getting through for as long (the default is a minute).
pub const REASSEMBLY_TIMEOUT: Duration = Duration::from_millis(500);

/// Give the 802.15.4 interface `node`'s address, and the PAN and 6LoWPAN
/// context the others use
pub fn configure_radio_iface(iface: &Iface<'_>, node: Node) {
    iface.stack().set_reassembly_timeout(REASSEMBLY_TIMEOUT);
    iface.set_pan_id(Some(Ieee802154Pan(PAN_ID)));
    iface
        .set_sixlowpan_address_context([SixlowpanAddressContext(PREFIX)])
        .unwrap();
    iface
        .add_ip_addr(IpCidr::V6(Ipv6Cidr::new(node.ip(), PREFIX_LEN)))
        .unwrap();
}

/// Keep the other nodes in the neighbor cache
///
/// Every node's link layer address follows from its IP, so there's no need
/// to discover them. Neighbor solicitations are broadcast, and broadcasts
/// aren't acked or retried, so relying on them makes connecting flaky.
pub async fn keep_neighbors(stack: Stack<'_>, iface: &Iface<'_>, us: Node) -> ! {
    // the stack pushes entries back to a minute out when it hears from them,
    // so refresh well within that
    const LIFETIME: Duration = Duration::from_secs(60);
    const REFRESH: Duration = Duration::from_secs(20);

    loop {
        for node in Node::ALL.into_iter().filter(|n| *n != us) {
            for ip in [node.link_local(), node.ip()] {
                _ = stack.neighbor_cache().insert(
                    iface.handle(),
                    IpAddr::V6(ip),
                    HardwareAddress::Ieee802154(Ieee802154Address::Extended(node.mac())),
                    Instant::now() + LIFETIME,
                );
            }
        }

        Timer::after(REFRESH).await;
    }
}
