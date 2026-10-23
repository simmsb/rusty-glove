//! Reliable, ordered messages to one other node, over UDP, see
//! [`shared::reliable`]
//!
//! [`Link::run`] must be running for anything else to work: it takes packets
//! off the socket, acks messages and sends heartbeats. Messages it passes on
//! wait in a queue for [`Link::recv`], and while that's full new ones aren't
//! acked, so the sender holds on to them.

use core::{
    cell::{Cell, RefCell},
    marker::PhantomData,
};

use embassy_futures::{
    join::join,
    select::{select, Either},
};
use embassy_net::{udp::UdpSocket, wire::SocketAddr};
use embassy_sync::{blocking_mutex::raw::NoopRawMutex, channel::Channel, signal::Signal};
use embassy_time::{Duration, Instant, Timer};
use serde::{de::DeserializeOwned, Serialize};
use shared::reliable::{Packet, Peer};

use crate::{HEARTBEAT, LINK_TIMEOUT};

macro_rules! info {
    ($($t:tt)*) => {{
        #[cfg(feature = "defmt")]
        defmt::info!($($t)*);
    }};
}

macro_rules! warn {
    ($($t:tt)*) => {{
        #[cfg(feature = "defmt")]
        defmt::warn!($($t)*);
    }};
}

/// How long to wait for the first ack. The radio has given up on a frame
/// well within this, so a resend doesn't queue up behind the first copy.
const RETRY_FIRST: Duration = Duration::from_millis(25);
/// Waits double after each try up to this
const RETRY_MAX: Duration = Duration::from_millis(200);

/// The message couldn't be delivered: the other end didn't ack it within
/// [`LINK_TIMEOUT`]
#[derive(Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Undelivered;

struct State {
    peer: Peer,
    up: bool,
    /// Counts the times the link has come up, see [`Link::wait_up`]
    session: u32,
    last_heard: Instant,
    last_sent: Instant,
}

/// The link to one other node, sending `Tx` and receiving `Rx`. `N` must fit
/// an encoded packet of either: `POSTCARD_MAX_SIZE +
/// shared::reliable::HEADER_LEN`. `Q` is how many received messages can wait
/// for [`Link::recv`].
///
/// Everything here must run in one task, it isn't `Sync`.
pub struct Link<'d, Tx, Rx, const N: usize, const Q: usize> {
    socket: UdpSocket<'d>,
    peer: SocketAddr,
    epoch: u32,
    next_seq: Cell<u16>,
    rng: Cell<u32>,
    state: RefCell<State>,
    /// The latest seq acked
    acked: Signal<NoopRawMutex, u16>,
    /// The link went up or down
    changed: Signal<NoopRawMutex, ()>,
    inbox: Channel<NoopRawMutex, Rx, Q>,
    _tx: PhantomData<Tx>,
}

impl<'d, Tx: Serialize, Rx: DeserializeOwned, const N: usize, const Q: usize>
    Link<'d, Tx, Rx, N, Q>
{
    /// Talk to `peer` with `socket`, which must be bound to `peer` alone so
    /// everything it receives is from it. `epoch` should be random, and
    /// different every boot.
    pub fn new(socket: UdpSocket<'d>, peer: impl Into<SocketAddr>, epoch: u32) -> Self {
        Self {
            socket,
            peer: peer.into(),
            epoch,
            // starting somewhere random means an ack from before we rebooted
            // is unlikely to match
            next_seq: Cell::new(epoch as u16),
            rng: Cell::new(epoch | 1),
            state: RefCell::new(State {
                peer: Peer::new(),
                up: false,
                session: 0,
                last_heard: Instant::MIN,
                last_sent: Instant::MIN,
            }),
            acked: Signal::new(),
            changed: Signal::new(),
            inbox: Channel::new(),
            _tx: PhantomData,
        }
    }

    /// Whether we've heard from the other end within [`LINK_TIMEOUT`]
    pub fn is_up(&self) -> bool {
        self.state.borrow().up
    }

    /// Wait for the link to be up, returning which time it is it's come up
    ///
    /// Only one task may wait on the link going up or down at a time.
    pub async fn wait_up(&self) -> u32 {
        loop {
            {
                let state = self.state.borrow();
                if state.up {
                    return state.session;
                }
            }
            self.changed.wait().await;
        }
    }

    /// Wait for the link that came up as `session` to go down, which it also
    /// does if the other end reboots
    pub async fn wait_down(&self, session: u32) {
        loop {
            {
                let state = self.state.borrow();
                if !state.up || state.session != session {
                    return;
                }
            }
            self.changed.wait().await;
        }
    }

    /// The next message from the other end
    pub async fn recv(&self) -> Rx {
        self.inbox.receive().await
    }

    /// Send a message and wait for it to be acked, trying for
    /// [`LINK_TIMEOUT`]
    pub async fn send(&self, msg: Tx) -> Result<(), Undelivered> {
        let seq = self.next_seq.get();
        self.next_seq.set(seq.wrapping_add(1));

        let packet = Packet::Data {
            epoch: self.epoch,
            seq,
            msg,
        };
        let mut buf = [0u8; N];
        let encoded = postcard::to_slice(&packet, &mut buf).expect("message too large for N");

        self.acked.reset();
        let start = Instant::now();
        let mut wait = RETRY_FIRST;
        loop {
            self.transmit(encoded).await;

            let ack = async {
                while self.acked.wait().await != seq {}
            };
            if let Either::First(()) = select(ack, Timer::after(self.jitter(wait))).await {
                return Ok(());
            }

            if start.elapsed() >= LINK_TIMEOUT {
                return Err(Undelivered);
            }
            wait = (wait * 2).min(RETRY_MAX);
        }
    }

    /// Run the link, see the module docs
    pub async fn run(&self) -> ! {
        join(self.receive(), self.keep_alive()).await;
        unreachable!()
    }

    async fn receive(&self) -> ! {
        let mut buf = [0u8; N];
        loop {
            let n = match self.socket.recv_from(&mut buf).await {
                Ok((n, _)) => n,
                Err(e) => {
                    warn!("link: receive failed: {}", e);
                    continue;
                }
            };
            let Ok(packet) = postcard::from_bytes::<Packet<Rx>>(&buf[..n]) else {
                warn!("link: dropping a packet that doesn't decode");
                continue;
            };

            self.heard(packet.epoch());

            match packet {
                Packet::Data { seq, msg, .. } => {
                    let duplicate = self.state.borrow().peer.is_duplicate(seq);
                    if !duplicate {
                        if self.inbox.try_send(msg).is_err() {
                            // not acking it means it'll be sent again
                            continue;
                        }
                        self.state.borrow_mut().peer.delivered(seq);
                    }

                    let ack = Packet::<()>::Ack {
                        epoch: self.epoch,
                        seq,
                    };
                    let mut ack_buf = [0u8; shared::reliable::HEADER_LEN];
                    if let Ok(encoded) = postcard::to_slice(&ack, &mut ack_buf) {
                        self.transmit(encoded).await;
                    }
                }
                Packet::Ack { seq, .. } => self.acked.signal(seq),
                Packet::Heartbeat { .. } => {}
            }
        }
    }

    /// Send heartbeats when there's been nothing else to send, and notice
    /// when the other end stops sending anything
    async fn keep_alive(&self) -> ! {
        loop {
            let (heartbeat_at, timeout_at) = {
                let state = self.state.borrow();
                (
                    state.last_sent + HEARTBEAT,
                    state.up.then(|| state.last_heard + LINK_TIMEOUT),
                )
            };

            let now = Instant::now();
            if now >= heartbeat_at {
                let heartbeat = Packet::<()>::Heartbeat { epoch: self.epoch };
                let mut buf = [0u8; shared::reliable::HEADER_LEN];
                if let Ok(encoded) = postcard::to_slice(&heartbeat, &mut buf) {
                    self.transmit(encoded).await;
                }
                continue;
            }

            if timeout_at.is_some_and(|t| now >= t) {
                info!("link: {} went quiet", self.peer);
                self.state.borrow_mut().up = false;
                self.changed.signal(());
                continue;
            }

            // either may move later while we wait, then this wakes early
            Timer::at(timeout_at.map_or(heartbeat_at, |t| t.min(heartbeat_at))).await;
        }
    }

    fn heard(&self, epoch: u32) {
        let mut state = self.state.borrow_mut();
        state.last_heard = Instant::now();

        let rebooted = state.peer.heard(epoch);
        if rebooted {
            info!("link: {} rebooted", self.peer);
        }
        if !state.up || rebooted {
            if !state.up {
                info!("link: {} is up", self.peer);
            }
            state.up = true;
            state.session = state.session.wrapping_add(1);
            self.changed.signal(());
        }
    }

    async fn transmit(&self, packet: &[u8]) {
        self.state.borrow_mut().last_sent = Instant::now();
        // a lost packet is sent again, or doesn't matter
        if let Err(e) = self.socket.send_to(packet, self.peer).await {
            warn!("link: send failed: {}", e);
        }
    }

    /// `d` plus up to a quarter more, so two ends that lost packets at the
    /// same time don't resend at the same time again
    fn jitter(&self, d: Duration) -> Duration {
        // xorshift32
        let mut x = self.rng.get();
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng.set(x);

        let quarter = d.as_micros() / 4;
        d + Duration::from_micros(x as u64 % (quarter + 1))
    }
}
