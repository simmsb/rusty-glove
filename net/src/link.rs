//! Reliable, ordered messages to one other node, over UDP, see
//! [`shared::reliable`]
//!
//! [`Link::run`] must be running for anything else to work: it takes packets
//! off the socket, acks them and sends heartbeats. Messages it passes on wait
//! in a queue for [`Link::recv`], and while there's no room for a batch it
//! isn't acked, so the sender holds on to it.

use core::{
    cell::{Cell, RefCell},
    marker::PhantomData,
};

use core::future::Future;

use embassy_futures::{
    join::join3,
    select::{select, Either},
};
use embassy_net::{udp::UdpSocket, wire::SocketAddr};
use embassy_sync::{
    blocking_mutex::{
        raw::{CriticalSectionRawMutex, NoopRawMutex},
        Mutex,
    },
    channel::Channel,
    signal::Signal,
};
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

/// How often [`Link::run`] logs its [`Stats`]
const STATS_PERIOD: Duration = Duration::from_secs(10);

/// The batch couldn't be delivered: the other end didn't ack it within
/// [`LINK_TIMEOUT`]
#[derive(Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Undelivered;

/// Counts of what a link has been up to, to see how it's doing without
/// logging every packet
#[derive(Default, Clone, Copy, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Stats {
    /// Batches sent
    pub sent: u32,
    /// Of those, acked before they needed sending again
    pub first_try: u32,
    /// Times a batch was sent again
    pub resends: u32,
    /// Batches given up on
    pub undelivered: u32,
    /// Batches from the other end turned away, as there was no room for them
    /// in the queue for [`Link::recv`]
    pub refused: u32,
    /// Batches from the other end that we'd already passed on
    pub duplicates: u32,
    /// The longest wait for an ack since the last log, in ms
    pub max_ack_ms: u32,
    /// The longest wait for an ack since [`LinkStats::read`], in ms
    pub max_ack_ms_since_read: u32,
    /// Whether we're hearing from the other end
    pub up: bool,
    /// Times the other end went quiet
    pub downs: u32,
    /// Times we heard the other end had rebooted
    pub peer_reboots: u32,
}

/// Where a link keeps its [`Stats`], so they can be read from outside the
/// task running it
pub struct LinkStats(Mutex<CriticalSectionRawMutex, Cell<Stats>>);

impl LinkStats {
    pub const fn new() -> Self {
        Self(Mutex::new(Cell::new(Stats {
            sent: 0,
            first_try: 0,
            resends: 0,
            undelivered: 0,
            refused: 0,
            duplicates: 0,
            max_ack_ms: 0,
            max_ack_ms_since_read: 0,
            up: false,
            downs: 0,
            peer_reboots: 0,
        })))
    }

    pub fn get(&self) -> Stats {
        self.0.lock(|s| s.get())
    }

    /// The stats, starting `max_ack_ms_since_read` over
    pub fn read(&self) -> Stats {
        let stats = self.get();
        self.update(|s| s.max_ack_ms_since_read = 0);
        stats
    }

    fn update(&self, f: impl FnOnce(&mut Stats)) {
        self.0.lock(|s| {
            let mut stats = s.get();
            f(&mut stats);
            s.set(stats);
        })
    }
}

impl Default for LinkStats {
    fn default() -> Self {
        Self::new()
    }
}

/// Wait for a message with `first`, then take any more `more` has ready
/// without waiting, up to a batch of `B`
pub async fn batch<T, const B: usize>(
    first: impl Future<Output = T>,
    mut more: impl FnMut() -> Option<T>,
) -> heapless::Vec<T, B> {
    let mut batch = heapless::Vec::new();
    _ = batch.push(first.await);
    while !batch.is_full() {
        let Some(msg) = more() else { break };
        _ = batch.push(msg);
    }
    batch
}

struct State {
    peer: Peer,
    up: bool,
    /// Counts the times the link has come up, see [`Link::wait_up`]
    session: u32,
    last_heard: Instant,
    last_sent: Instant,
}

/// The link to one other node, sending `Tx` and receiving `Rx` in batches.
/// `N` must fit an encoded packet of either, see
/// [`shared::reliable::packet_len`]. `Q` is how many received messages can
/// wait for [`Link::recv`]: no less than the other end's batches, which are
/// refused otherwise, and better twice that, so one can arrive while the last
/// is still being taken.
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
    stats: &'d LinkStats,
    _tx: PhantomData<Tx>,
}

impl<'d, Tx: Serialize, Rx: DeserializeOwned, const N: usize, const Q: usize>
    Link<'d, Tx, Rx, N, Q>
{
    /// Talk to `peer` with `socket`, which must be bound to `peer` alone so
    /// everything it receives is from it. `epoch` should be random, and
    /// different every boot.
    pub fn new(
        socket: UdpSocket<'d>,
        peer: impl Into<SocketAddr>,
        epoch: u32,
        stats: &'d LinkStats,
    ) -> Self {
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
            stats,
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

    /// Send a batch of messages and wait for it to be acked, trying for
    /// [`LINK_TIMEOUT`]
    pub async fn send(&self, msgs: &[Tx]) -> Result<(), Undelivered> {
        let seq = self.next_seq.get();
        self.next_seq.set(seq.wrapping_add(1));

        let packet = Packet::Data {
            epoch: self.epoch,
            seq,
            msgs,
        };
        let mut buf = [0u8; N];
        let encoded = postcard::to_slice(&packet, &mut buf).expect("batch too large for N");

        self.count(|s| s.sent += 1);
        self.acked.reset();
        let start = Instant::now();
        let mut wait = RETRY_FIRST;
        loop {
            self.transmit(encoded).await;

            let ack = async {
                while self.acked.wait().await != seq {}
            };
            if let Either::First(()) = select(ack, Timer::after(self.jitter(wait))).await {
                let waited = start.elapsed().as_millis() as u32;
                self.count(|s| {
                    if wait == RETRY_FIRST {
                        s.first_try += 1;
                    }
                    s.max_ack_ms = s.max_ack_ms.max(waited);
                    s.max_ack_ms_since_read = s.max_ack_ms_since_read.max(waited);
                });
                return Ok(());
            }

            if start.elapsed() >= LINK_TIMEOUT {
                self.count(|s| s.undelivered += 1);
                return Err(Undelivered);
            }
            self.count(|s| s.resends += 1);
            wait = (wait * 2).min(RETRY_MAX);
        }
    }

    /// Run the link, see the module docs
    pub async fn run(&self) -> ! {
        join3(self.receive(), self.keep_alive(), self.report()).await;
        unreachable!()
    }

    async fn report(&self) -> ! {
        loop {
            Timer::after(STATS_PERIOD).await;
            info!("link {}: {}", self.peer, self.stats.get());
            self.count(|s| s.max_ack_ms = 0);
        }
    }

    fn count(&self, f: impl FnOnce(&mut Stats)) {
        self.stats.update(f);
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
            let Ok(packet) = postcard::from_bytes::<Packet<heapless::Vec<Rx, Q>>>(&buf[..n]) else {
                warn!("link: dropping a packet that doesn't decode");
                continue;
            };

            self.heard(packet.epoch());

            match packet {
                Packet::Data { seq, msgs, .. } => {
                    let duplicate = self.state.borrow().peer.is_duplicate(seq);
                    if duplicate {
                        self.count(|s| s.duplicates += 1);
                    } else {
                        if self.inbox.free_capacity() < msgs.len() {
                            // not acking it means it'll be sent again
                            self.count(|s| s.refused += 1);
                            continue;
                        }
                        for msg in msgs {
                            _ = self.inbox.try_send(msg);
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
                self.count(|s| {
                    s.up = false;
                    s.downs += 1;
                });
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
            self.count(|s| s.peer_reboots += 1);
        }
        if !state.up || rebooted {
            if !state.up {
                info!("link: {} is up", self.peer);
            }
            state.up = true;
            self.count(|s| s.up = true);
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
