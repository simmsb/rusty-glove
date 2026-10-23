//! The network the halves and the dongle share, IPv6 over 6LoWPAN on IEEE
//! 802.15.4, plus the protocols spoken over it
//!
//! Every node must agree on the channel, PAN, prefix and 6LoWPAN context, so
//! they only live here.

use core::net::Ipv6Addr;

use serde::{Deserialize, Serialize};

/// 2475MHz, above wifi channel 11
pub const CHANNEL: u8 = 25;

pub const PAN_ID: u16 = 0x6c67;

/// fd67:6c76:0:1::/64, also 6LoWPAN address context 0 so it can be elided
pub const PREFIX: [u8; 8] = [0xfd, 0x67, 0x6c, 0x76, 0x00, 0x00, 0x00, 0x01];
pub const PREFIX_LEN: u8 = 64;

#[derive(Eq, PartialEq, Clone, Copy, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Node {
    Left = 1,
    Right = 2,
    Dongle = 3,
}

impl Node {
    pub const ALL: [Node; 3] = [Node::Left, Node::Right, Node::Dongle];

    pub const fn ip(self) -> Ipv6Addr {
        let p = PREFIX;
        Ipv6Addr::new(
            u16::from_be_bytes([p[0], p[1]]),
            u16::from_be_bytes([p[2], p[3]]),
            u16::from_be_bytes([p[4], p[5]]),
            u16::from_be_bytes([p[6], p[7]]),
            0,
            0,
            0,
            self as u16,
        )
    }

    /// The address the nodes use to talk to each other
    ///
    /// The network stack only compresses link-local addresses whose interface
    /// identifier comes from the link layer address, which saves 32 bytes, a
    /// quarter of a frame, on every frame. [`Node::ip`] is still assigned.
    pub const fn link_local(self) -> Ipv6Addr {
        Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, self as u16)
    }

    /// The extended 802.15.4 address
    ///
    /// The interface identifier of [`Node::ip`] is derived from this (RFC
    /// 4944: flip the universal/local bit), so 6LoWPAN can elide addresses
    /// entirely.
    pub const fn mac(self) -> [u8; 8] {
        [0x02, 0, 0, 0, 0, 0, 0, self as u8]
    }
}

/// The link between the halves, and the right half and the dongle, are UDP
/// with the same port at both ends, see [`crate::reliable`]. Firmware updates
/// are TCP.
pub mod port {
    /// Between the halves
    pub const INTERBOARD: u16 = 7100;
    /// Between the right half and the dongle, keyboard reports and what OS
    /// the host is
    pub const KEYBOARD: u16 = 7200;
    /// On each half, and on the dongle's USB side, see [`super::dfu`]
    pub const DFU: u16 = 7300;
    /// On the dongle's USB side, forwarded to [`DFU`] on the left half
    pub const DFU_LEFT: u16 = 7301;
    /// On the dongle's USB side, forwarded to [`DFU`] on the right half
    pub const DFU_RIGHT: u16 = 7302;
}

/// The network between the dongle and its host, over USB ethernet
pub mod usb {
    use core::net::Ipv4Addr;

    pub const DONGLE_IP: Ipv4Addr = Ipv4Addr::new(10, 42, 0, 1);
    pub const PREFIX_LEN: u8 = 24;
    /// Handed out over DHCP
    pub const POOL_START: Ipv4Addr = Ipv4Addr::new(10, 42, 0, 2);
    pub const POOL_END: Ipv4Addr = Ipv4Addr::new(10, 42, 0, 9);

    /// Locally administered unicast
    pub const DONGLE_MAC: [u8; 6] = [0x02, 0x67, 0x6c, 0x76, 0x00, 0x01];
    /// What the host's end of the link will use
    pub const HOST_MAC: [u8; 6] = [0x02, 0x67, 0x6c, 0x76, 0x00, 0x02];
}

/// The buffer a [`crate::reliable`] link sending messages of up to `tx` bytes
/// and receiving up to `rx` bytes needs
pub const fn max_packet_len(tx: usize, rx: usize) -> usize {
    (if tx > rx { tx } else { rx }) + crate::reliable::HEADER_LEN
}

/// Size of a packed `NKROBootKeyboardReport`
pub const KEYBOARD_REPORT_LEN: usize = 25;

#[derive(
    Serialize, Deserialize, Eq, PartialEq, Clone, Debug, postcard::experimental::max_size::MaxSize,
)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum KeyboardToDongle {
    /// A packed keyboard report, the dongle passes this straight to the host
    Report([u8; KEYBOARD_REPORT_LEN]),
}

#[derive(
    Serialize,
    Deserialize,
    Eq,
    PartialEq,
    Clone,
    Copy,
    Debug,
    postcard::experimental::max_size::MaxSize,
)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum HostOs {
    Windows,
    Linux,
    MacOS,
    Unknown,
}

#[derive(
    Serialize, Deserialize, Eq, PartialEq, Clone, Debug, postcard::experimental::max_size::MaxSize,
)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum DongleToKeyboard {
    /// The OS the dongle guesses it's plugged into, used for unicode input
    HostOs(HostOs),
}

/// The firmware update protocol, spoken on [`port::DFU`]
///
/// The client sends [`MAGIC`] and an op byte, the device answers with a
/// [`Status`] byte and closes the connection.
///
/// - [`OP_WRITE`]: followed by the image length and its CRC32 (IEEE), both
///   little endian u32s, then the image itself. The image is the application
///   as a flat binary. Once it's written and checks out the device answers,
///   then reboots into it.
/// - [`OP_CONFIRM`]: keep the running image. A freshly updated image that
///   isn't confirmed is rolled back the next time the device resets, so an
///   image that can't be reached over the network can't stick. Writes are
///   refused with [`Status::Unconfirmed`] until then.
pub mod dfu {
    pub const MAGIC: [u8; 4] = *b"GDFU";

    pub const OP_WRITE: u8 = 0;
    pub const OP_CONFIRM: u8 = 1;

    #[derive(Eq, PartialEq, Clone, Copy, Debug)]
    #[cfg_attr(feature = "defmt", derive(defmt::Format))]
    #[repr(u8)]
    pub enum Status {
        Ok = 0,
        BadRequest = 1,
        TooLarge = 2,
        CrcMismatch = 3,
        FlashError = 4,
        /// The running image needs confirming before it can be replaced
        Unconfirmed = 5,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses() {
        assert_eq!(
            Node::Right.ip(),
            "fd67:6c76:0:1::2".parse::<Ipv6Addr>().unwrap()
        );

        // the interface identifier is the mac with the U/L bit flipped
        for node in Node::ALL {
            let mut iid = node.mac();
            iid[0] ^= 0x02;
            assert_eq!(&node.ip().octets()[8..], &iid);
            assert_eq!(&node.link_local().octets()[8..], &iid);
            assert!(node.link_local().is_unicast_link_local());
        }

        assert!(usb::DONGLE_IP.octets()[..3] == usb::POOL_START.octets()[..3]);
        assert!(usb::POOL_END.octets()[..3] == usb::POOL_START.octets()[..3]);
    }
}
