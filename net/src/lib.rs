#![no_std]

//! The 802.15.4 network shared by the halves and the dongle

use embassy_net::{
    iface::Iface,
    tcp::TcpSocket,
    wire::{
        HardwareAddress, Ieee802154Address, Ieee802154Pan, IpAddr, IpCidr, Ipv6Cidr,
        SixlowpanAddressContext,
    },
    Stack,
};
use embassy_time::{Duration, Instant, Timer};
use shared::net::{Node, PAN_ID, PREFIX, PREFIX_LEN};

pub mod framed;
pub mod radio;

/// How long a link can go without hearing from the other end
pub const LINK_TIMEOUT: Duration = Duration::from_secs(3);
/// How often the connecting end of a link sends something, if it has had
/// nothing else to send
pub const HEARTBEAT: Duration = Duration::from_secs(1);

/// Which end of a link a socket is
///
/// Only the connecting end sends heartbeats, as data, so both ends hear from
/// each other (the heartbeat, and its ack) without sending at the same time.
/// TCP keep-alives don't do for this: a keep-alive doesn't count as hearing
/// from the peer, and the end whose keep-alive fires first holds back the
/// other end's, so the other end times out. The accepting end must notice
/// the heartbeats stopping itself, see [`LINK_TIMEOUT`].
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum LinkEnd {
    Connecting,
    Accepting,
}

/// Give the 802.15.4 interface `node`'s address, and the PAN and 6LoWPAN
/// context the others use
pub fn configure_radio_iface(iface: &Iface<'_>, node: Node) {
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

/// Set up a socket for a long lived link that carries small messages which
/// should go out straight away, and notice when the other end goes away
///
/// Acks are left delayed: our messages don't wait on them with Nagle off,
/// and an ack frame for every message would double the frames on air.
pub fn tune_link_socket(socket: &mut TcpSocket<'_, '_>, end: LinkEnd) {
    socket.set_nagle_enabled(false);
    // with heartbeats going out this never fires, it's for while connecting
    if end == LinkEnd::Connecting {
        socket.set_keep_alive(Some(HEARTBEAT));
    }
    socket.set_timeout(Some(LINK_TIMEOUT));
}
