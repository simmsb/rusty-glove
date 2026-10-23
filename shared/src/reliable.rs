//! Reliable, ordered messages over datagrams, for the links between the
//! halves and the dongle
//!
//! Stop and wait: each message has a sequence number, the receiver acks it
//! with that number, and the sender resends until it's acked before it moves
//! on to the next message. The receiver passes on a message only if its
//! sequence number differs from the last one it passed on, so a resend whose
//! first copy got through (but whose ack didn't) isn't seen twice.
//!
//! Every packet carries the sender's epoch, picked at random when it boots, so
//! the other end notices it rebooted even if that was quicker than the link
//! times out, and doesn't mistake its first message for a resend.

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Eq, PartialEq, Clone, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Packet<T> {
    Data { epoch: u32, seq: u16, msg: T },
    /// The receiver of [`Packet::Data`] sends this back with its `seq`, and
    /// its own epoch
    Ack { epoch: u32, seq: u16 },
    /// Sent when there's been nothing else to send for a while, so the other
    /// end knows we're still here
    Heartbeat { epoch: u32 },
}

impl<T> Packet<T> {
    pub fn epoch(&self) -> u32 {
        match self {
            Packet::Data { epoch, .. } | Packet::Ack { epoch, .. } | Packet::Heartbeat { epoch } => {
                *epoch
            }
        }
    }
}

/// The most a packet adds to the encoded message: the variant, and the epoch
/// and sequence number as varints
pub const HEADER_LEN: usize = 1 + 5 + 3;

/// What the receiving end knows about the sending end
#[derive(Default)]
pub struct Peer {
    epoch: Option<u32>,
    last: Option<u16>,
}

impl Peer {
    pub const fn new() -> Self {
        Self {
            epoch: None,
            last: None,
        }
    }

    /// Note a packet arrived with `epoch`, returning whether the peer has
    /// rebooted since we last heard from it
    pub fn heard(&mut self, epoch: u32) -> bool {
        match self.epoch.replace(epoch) {
            Some(old) if old == epoch => false,
            old => {
                self.last = None;
                old.is_some()
            }
        }
    }

    /// Whether a message with `seq` has been passed on already, it should
    /// still be acked
    pub fn is_duplicate(&self, seq: u16) -> bool {
        self.last == Some(seq)
    }

    /// Note the message with `seq` has been passed on
    pub fn delivered(&mut self, seq: u16) {
        self.last = Some(seq);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicates() {
        let mut peer = Peer::new();
        assert!(!peer.heard(7));
        assert!(!peer.is_duplicate(0));
        peer.delivered(0);

        assert!(!peer.heard(7));
        assert!(peer.is_duplicate(0));
        assert!(!peer.is_duplicate(1));
        peer.delivered(1);
        assert!(!peer.is_duplicate(0));
        assert!(peer.is_duplicate(1));
    }

    #[test]
    fn reboot() {
        let mut peer = Peer::new();
        peer.heard(7);
        peer.delivered(3);

        // the rebooted peer may well land on the same sequence number
        assert!(peer.heard(8));
        assert!(!peer.is_duplicate(3));
        assert!(!peer.heard(8));
    }

    #[test]
    fn header_len() {
        let packets = [
            Packet::Data {
                epoch: u32::MAX,
                seq: u16::MAX,
                msg: [0xffu8; 25],
            },
            Packet::Ack {
                epoch: u32::MAX,
                seq: u16::MAX,
            },
            Packet::Heartbeat { epoch: u32::MAX },
        ];

        for packet in packets {
            let mut buf = [0u8; 64];
            let encoded = postcard::to_slice(&packet, &mut buf).unwrap();
            assert!(encoded.len() <= HEADER_LEN + 25);
            assert_eq!(postcard::from_bytes::<Packet<[u8; 25]>>(encoded).unwrap(), packet);
        }
    }
}
