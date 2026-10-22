//! embassy-net driver for the nRF52840's radio in 802.15.4 mode
//!
//! Like embassy-nrf's own driver, except that it sets the channel and power,
//! doesn't panic when the channel is busy, and acks and retransmits unicast
//! frames. The network stack never asks for acks, and without them every lost
//! frame costs a TCP retransmission.
//!
//! Acks only work if both ends react within a millisecond or so, so the radio
//! is driven by [`Mac`], which should run on a high priority executor where
//! the rest of the firmware can't hold it up. The network stack's side of the
//! driver isn't safe to use from another priority, so [`Bridge`] passes frames
//! between the two from thread mode.

use embassy_futures::{
    join::join,
    select::{select, Either},
};
use embassy_net_driver_channel::{
    self as ch,
    driver::{HardwareAddress, LinkState},
};
use core::pin::pin;

use embassy_nrf::{
    pac,
    radio::ieee802154::{Packet, Radio},
};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel};
use embassy_time::{with_timeout, Duration, Instant, Timer};
use portable_atomic::{AtomicU32, Ordering};
use shared::{
    mac::{self, Address, Header},
    net::CHANNEL,
};

pub const MTU: usize = Packet::CAPACITY as usize;

macro_rules! trace {
    ($($t:tt)*) => {{
        #[cfg(feature = "defmt")]
        defmt::trace!($($t)*);
    }};
}

const TX_POWER: i8 = 8;
/// Tries at getting a unicast frame acked, or through a busy channel
const ATTEMPTS: usize = 4;
/// An ack turns up about 670us after we finish sending
const ACK_TIMEOUT: Duration = Duration::from_micros(1000);
/// The 802.15.4 backoff period
const BACKOFF_UNIT_US: u64 = 320;
/// The longest frame takes about 4.3ms to arrive
const MAX_FRAME_TIME: Duration = Duration::from_millis(5);

/// Counts of what the radio has been up to, to see how well the link is doing
/// without logging every frame
#[derive(Default, Clone, Copy, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Stats {
    /// Unicast frames we sent
    pub sent: u32,
    /// ... that were acked on the first try
    pub acked_first: u32,
    /// ... that were acked after retrying
    pub acked_retry: u32,
    /// ... that were never acked
    pub gave_up: u32,
    /// Tries that found the channel busy
    pub busy: u32,
    /// Frames received for us
    pub received: u32,
    /// ... that were retransmissions of ones we'd already acked
    pub duplicates: u32,
    /// ... that the stack had no room for
    pub dropped: u32,
    /// Mean time from starting to send a unicast frame to its ack
    pub ack_time_avg_us: u32,
    /// Longest time from starting to send a unicast frame to its ack
    pub ack_time_max_us: u32,
}

struct AtomicStats {
    sent: AtomicU32,
    acked_first: AtomicU32,
    acked_retry: AtomicU32,
    gave_up: AtomicU32,
    busy: AtomicU32,
    received: AtomicU32,
    duplicates: AtomicU32,
    dropped: AtomicU32,
    ack_time_total_us: AtomicU32,
    ack_time_max_us: AtomicU32,
}

static STATS: AtomicStats = AtomicStats {
    sent: AtomicU32::new(0),
    acked_first: AtomicU32::new(0),
    acked_retry: AtomicU32::new(0),
    gave_up: AtomicU32::new(0),
    busy: AtomicU32::new(0),
    received: AtomicU32::new(0),
    duplicates: AtomicU32::new(0),
    dropped: AtomicU32::new(0),
    ack_time_total_us: AtomicU32::new(0),
    ack_time_max_us: AtomicU32::new(0),
};

fn count(counter: &AtomicU32) {
    counter.fetch_add(1, Ordering::Relaxed);
}

/// What the radio has done since startup
pub fn stats() -> Stats {
    let s = &STATS;
    let get = |c: &AtomicU32| c.load(Ordering::Relaxed);
    let acked = get(&s.acked_first) + get(&s.acked_retry);
    Stats {
        sent: get(&s.sent),
        acked_first: get(&s.acked_first),
        acked_retry: get(&s.acked_retry),
        gave_up: get(&s.gave_up),
        busy: get(&s.busy),
        received: get(&s.received),
        duplicates: get(&s.duplicates),
        dropped: get(&s.dropped),
        ack_time_avg_us: get(&s.ack_time_total_us).checked_div(acked).unwrap_or(0),
        ack_time_max_us: get(&s.ack_time_max_us),
    }
}

type Frame = heapless::Vec<u8, MTU>;
/// Frames waiting between [`Bridge`] and [`Mac`], each way
const QUEUE_LEN: usize = 4;
type FrameChannel = Channel<CriticalSectionRawMutex, Frame, QUEUE_LEN>;

pub type Device<'d> = ch::Device<'d>;

pub struct State<const N_RX: usize, const N_TX: usize> {
    ch: ch::State<N_RX, N_TX>,
    to_air: FrameChannel,
    from_air: FrameChannel,
}

impl<const N_RX: usize, const N_TX: usize> State<N_RX, N_TX> {
    pub const fn new() -> Self {
        Self {
            ch: ch::State::new(),
            to_air: Channel::new(),
            from_air: Channel::new(),
        }
    }
}

impl<const N_RX: usize, const N_TX: usize> Default for State<N_RX, N_TX> {
    fn default() -> Self {
        Self::new()
    }
}

/// Passes frames between the network stack and [`Mac`], run it in thread mode
pub struct Bridge<'d> {
    ch: ch::Runner<'d>,
    to_air: &'d FrameChannel,
    from_air: &'d FrameChannel,
}

/// Drives the radio, run it on a high priority executor
pub struct Mac<'d> {
    radio: Radio<'d>,
    mac: [u8; 8],
    rng: u32,
    /// The last frame we acked, so we don't pass on retransmissions of it if
    /// the ack got lost
    last_acked: Option<(Address, u8)>,
    to_air: &'d FrameChannel,
    from_air: &'d FrameChannel,
}

/// `seed` picks the backoffs, so it must differ between nodes
pub fn new<'d, const N_RX: usize, const N_TX: usize>(
    mac: [u8; 8],
    mut radio: Radio<'d>,
    seed: u32,
    state: &'d mut State<N_RX, N_TX>,
) -> (Device<'d>, Bridge<'d>, Mac<'d>) {
    radio.set_channel(CHANNEL);
    radio.set_transmission_power(TX_POWER);

    let (runner, device) = ch::new(&mut state.ch, HardwareAddress::Ieee802154(mac), MTU);
    let (to_air, from_air) = (&state.to_air, &state.from_air);

    (
        device,
        Bridge {
            ch: runner,
            to_air,
            from_air,
        },
        Mac {
            radio,
            mac,
            // xorshift gets stuck on zero
            rng: seed | 1,
            last_acked: None,
            to_air,
            from_air,
        },
    )
}

impl Bridge<'_> {
    pub async fn run(self) -> ! {
        let (state, mut rx_chan, mut tx_chan) = self.ch.split();
        state.set_link_state(LinkState::Up);

        let tx = async {
            loop {
                let buf = tx_chan.tx().await;
                let len = buf.len().min(MTU);
                let frame = Frame::from_slice(&buf[..len]).unwrap();
                drop(buf);
                self.to_air.send(frame).await;
            }
        };

        let rx = async {
            loop {
                let frame = self.from_air.receive().await;
                let mut buf = rx_chan.rx_buf().await;
                let len = buf.capacity().min(frame.len());
                buf.set_len(len);
                buf.copy_from_slice(&frame[..len]);
                rx_chan.rx(buf).await;
            }
        };

        join(tx, rx).await;
        unreachable!()
    }
}

/// What we heard while listening
enum Heard {
    Ack(u8),
    Other,
}

impl Mac<'_> {
    pub async fn run(mut self) -> ! {
        let mut packet = Packet::new();

        loop {
            match select(self.radio.receive(&mut packet), self.to_air.receive()).await {
                Either::First(Ok(())) => {
                    if let Heard::Ack(_seq) = self.handle_rx(&packet).await {
                        trace!("mac: late ack seq={}", _seq);
                    }
                }
                Either::First(Err(_e)) => trace!("mac: rx error {}", _e),
                Either::Second(mut frame) => self.transmit(&mut frame).await,
            }
        }
    }

    /// Deal with a received frame: ack it if it's for us, and pass it up
    /// unless it's a retransmission we've already passed up
    ///
    /// Whatever we're doing, we must keep doing this whenever the radio is
    /// listening. If we only listened for our own acks while we have a frame
    /// to send, two nodes with frames for each other would never hear each
    /// other.
    async fn handle_rx(&mut self, packet: &Packet) -> Heard {
        let Some(header) = Header::parse(packet) else {
            trace!("mac: rx len={} unparsed", packet.len());
            self.pass_up(packet);
            return Heard::Other;
        };

        if header.frame_type == mac::FRAME_TYPE_ACK {
            return Heard::Ack(header.seq);
        }

        let for_us = header.dst == Address::Extended(self.mac);

        // the sender only waits a millisecond, so do this before anything else
        let _acked = if header.ack_request && for_us {
            let mut ack = Packet::new();
            ack.copy_from_slice(&mac::ack_frame(header.seq));
            Some(self.radio.try_send(&mut ack).await.is_ok())
        } else {
            None
        };

        trace!(
            "mac: rx len={} seq={} for_us={} acked={}",
            packet.len(),
            header.seq,
            for_us,
            _acked
        );

        if for_us {
            count(&STATS.received);
        }

        if header.ack_request && for_us {
            let key = (header.src, header.seq);
            if self.last_acked == Some(key) {
                count(&STATS.duplicates);
                return Heard::Other;
            }
            self.last_acked = Some(key);
        }

        self.pass_up(packet);
        Heard::Other
    }

    fn pass_up(&mut self, packet: &Packet) {
        // if the stack is behind, drop it, it'll be retransmitted
        if self
            .from_air
            .try_send(Frame::from_slice(packet).unwrap())
            .is_err()
        {
            count(&STATS.dropped);
            trace!("mac: stack behind, dropped a frame");
        }
    }

    async fn transmit(&mut self, frame: &mut [u8]) {
        let wants_ack = Header::parse(frame).is_some_and(|h| h.is_unicast_data());
        if wants_ack {
            mac::set_ack_request(frame);
        }
        let seq = frame[2];

        let mut packet = Packet::new();
        packet.copy_from_slice(frame);

        if wants_ack {
            count(&STATS.sent);
        }
        let started = Instant::now();

        // whatever made us send may have made someone else send too (they
        // heard the same frame, or their timers started together)
        self.backoff(0, usize::MAX).await;

        for attempt in 0..ATTEMPTS {
            if self.radio.try_send(&mut packet).await.is_err() {
                count(&STATS.busy);
                trace!("mac: tx seq={} channel busy", seq);
                self.backoff(1, attempt).await;
                continue;
            }

            if !wants_ack {
                trace!("mac: tx len={} seq={} broadcast", frame.len(), seq);
                return;
            }

            if self.listen_until(Instant::now() + ACK_TIMEOUT, Some(seq)).await {
                let took = started.elapsed().as_micros() as u32;
                STATS.ack_time_total_us.fetch_add(took, Ordering::Relaxed);
                STATS.ack_time_max_us.fetch_max(took, Ordering::Relaxed);
                count(if attempt == 0 {
                    &STATS.acked_first
                } else {
                    &STATS.acked_retry
                });
                trace!("mac: tx len={} seq={} acked, attempt {}", frame.len(), seq, attempt);
                return;
            }

            trace!("mac: tx len={} seq={} no ack, attempt {}", frame.len(), seq, attempt);
            self.backoff(1, attempt).await;
        }

        if wants_ack {
            count(&STATS.gave_up);
        }
        trace!("mac: tx seq={} gave up", seq);
    }

    /// Listen until `deadline`, dealing with whatever turns up. Returns true
    /// if we heard the ack for `ack`.
    ///
    /// A frame that has started arriving by the deadline is still received,
    /// as it's likely for us and frames take longer to arrive than we listen
    /// between retries. Cutting it off would lose it, and if the sender is
    /// retrying too we'd keep cutting off each other's frames.
    async fn listen_until(&mut self, deadline: Instant, ack: Option<u8>) -> bool {
        let r = pac::RADIO;
        let mut packet = Packet::new();

        loop {
            r.events_framestart().write_value(0);

            let received = {
                let mut rx = pin!(self.radio.receive(&mut packet));
                match select(Timer::at(deadline), rx.as_mut()).await {
                    Either::Second(res) => Some(res),
                    Either::First(()) if r.events_framestart().read() != 0 => {
                        with_timeout(MAX_FRAME_TIME, rx).await.ok()
                    }
                    Either::First(()) => None,
                }
            };

            match received {
                None => return false,
                Some(Ok(())) => {
                    if let Heard::Ack(got) = self.handle_rx(&packet).await {
                        if Some(got) == ack {
                            return true;
                        }
                    }
                }
                Some(Err(_)) => {}
            }

            if Instant::now() >= deadline {
                return false;
            }
        }
    }

    /// Wait a random number of backoff periods, at least `min`, from a window
    /// that doubles with each `attempt`, listening all the while, so two
    /// nodes that collided don't do it again
    async fn backoff(&mut self, min: u64, attempt: usize) {
        // xorshift32
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 17;
        self.rng ^= self.rng << 5;

        // the first try only spreads out a little, it's added to every frame
        let window = match attempt {
            usize::MAX => 4,
            n => 8u32 << n.min(2),
        };
        let periods = min + (self.rng % window) as u64;
        let deadline = Instant::now() + Duration::from_micros(BACKOFF_UNIT_US * periods);

        self.listen_until(deadline, None).await;
    }
}
