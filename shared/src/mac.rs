//! Just enough of the IEEE 802.15.4 MAC header to do acks and retransmissions
//!
//! The network stack builds the frames but never asks for acks, so the radio
//! driver turns them on for unicast frames and handles them itself. Only 2003
//! and 2006 frames are understood, which is what the stack sends.

pub const FRAME_TYPE_DATA: u8 = 0b001;
pub const FRAME_TYPE_ACK: u8 = 0b010;

const ACK_REQUEST: u16 = 1 << 5;
const PAN_ID_COMPRESSION: u16 = 1 << 6;

#[derive(Eq, PartialEq, Clone, Copy, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Address {
    Absent,
    Short(u16),
    /// Most significant byte first, the reverse of how it's sent
    Extended([u8; 8]),
}

impl Address {
    pub const BROADCAST: Self = Self::Short(0xffff);
}

#[derive(Eq, PartialEq, Clone, Copy, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Header {
    pub frame_type: u8,
    pub ack_request: bool,
    pub seq: u8,
    pub dst: Address,
    pub src: Address,
}

fn read_addr(frame: &[u8], offset: &mut usize, mode: u16) -> Option<Address> {
    let addr = match mode {
        0b00 => return Some(Address::Absent),
        0b10 => {
            let raw = frame.get(*offset..*offset + 2)?;
            *offset += 2;
            Address::Short(u16::from_le_bytes([raw[0], raw[1]]))
        }
        0b11 => {
            let mut raw: [u8; 8] = frame.get(*offset..*offset + 8)?.try_into().ok()?;
            *offset += 8;
            raw.reverse();
            Address::Extended(raw)
        }
        _ => return None,
    };

    Some(addr)
}

impl Header {
    pub fn parse(frame: &[u8]) -> Option<Self> {
        let fcf = u16::from_le_bytes([*frame.first()?, *frame.get(1)?]);
        let seq = *frame.get(2)?;

        let version = (fcf >> 12) & 0b11;
        if version > 1 {
            return None;
        }

        let dst_mode = (fcf >> 10) & 0b11;
        let src_mode = (fcf >> 14) & 0b11;

        let mut offset = 3;

        if dst_mode != 0 {
            offset += 2;
        }
        let dst = read_addr(frame, &mut offset, dst_mode)?;

        if src_mode != 0 && (fcf & PAN_ID_COMPRESSION == 0 || dst_mode == 0) {
            offset += 2;
        }
        let src = read_addr(frame, &mut offset, src_mode)?;

        Some(Self {
            frame_type: (fcf & 0b111) as u8,
            ack_request: fcf & ACK_REQUEST != 0,
            seq,
            dst,
            src,
        })
    }

    /// Whether this frame is worth acking: data sent to a single node
    pub fn is_unicast_data(&self) -> bool {
        self.frame_type == FRAME_TYPE_DATA
            && !matches!(self.dst, Address::Absent)
            && self.dst != Address::BROADCAST
    }
}

/// Ask the receiver to ack this frame
pub fn set_ack_request(frame: &mut [u8]) {
    frame[0] |= ACK_REQUEST as u8;
}

pub fn ack_frame(seq: u8) -> [u8; 3] {
    [FRAME_TYPE_ACK, 0, seq]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A frame laid out the way the network stack sends unicast data: 2003,
    /// PAN id compression, extended addresses
    fn unicast_frame(seq: u8, dst: [u8; 8], src: [u8; 8]) -> Vec<u8> {
        let fcf: u16 = FRAME_TYPE_DATA as u16 | PAN_ID_COMPRESSION | (0b11 << 10) | (0b11 << 14);
        let mut frame = fcf.to_le_bytes().to_vec();
        frame.push(seq);
        frame.extend_from_slice(&0x6c67u16.to_le_bytes());
        frame.extend(dst.iter().rev());
        frame.extend(src.iter().rev());
        frame.extend_from_slice(&[0x7a, 0x33, 0x3a]);
        frame
    }

    #[test]
    fn parses_unicast() {
        let dst = [0x02, 0, 0, 0, 0, 0, 0, 3];
        let src = [0x02, 0, 0, 0, 0, 0, 0, 2];
        let mut frame = unicast_frame(9, dst, src);

        let header = Header::parse(&frame).unwrap();
        assert_eq!(header.frame_type, FRAME_TYPE_DATA);
        assert_eq!(header.seq, 9);
        assert_eq!(header.dst, Address::Extended(dst));
        assert_eq!(header.src, Address::Extended(src));
        assert!(header.is_unicast_data());
        assert!(!header.ack_request);

        set_ack_request(&mut frame);
        assert!(Header::parse(&frame).unwrap().ack_request);
    }

    #[test]
    fn broadcast_isnt_unicast() {
        // neighbor solicitations go to the short broadcast address
        let fcf: u16 = FRAME_TYPE_DATA as u16 | PAN_ID_COMPRESSION | (0b10 << 10) | (0b11 << 14);
        let mut frame = fcf.to_le_bytes().to_vec();
        frame.push(1);
        frame.extend_from_slice(&0x6c67u16.to_le_bytes());
        frame.extend_from_slice(&[0xff, 0xff]);
        frame.extend_from_slice(&[1, 0, 0, 0, 0, 0, 0, 2]);

        let header = Header::parse(&frame).unwrap();
        assert_eq!(header.dst, Address::BROADCAST);
        assert_eq!(header.src, Address::Extended([2, 0, 0, 0, 0, 0, 0, 1]));
        assert!(!header.is_unicast_data());
    }

    #[test]
    fn parses_ack() {
        let header = Header::parse(&ack_frame(42)).unwrap();
        assert_eq!(header.frame_type, FRAME_TYPE_ACK);
        assert_eq!(header.seq, 42);
        assert_eq!(header.dst, Address::Absent);
    }

    #[test]
    fn rejects_truncated() {
        let frame = unicast_frame(1, [0; 8], [1; 8]);
        assert!(Header::parse(&frame[..12]).is_none());
        assert!(Header::parse(&frame[..2]).is_none());
    }
}
