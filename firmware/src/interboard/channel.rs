use embassy_sync::blocking_mutex::raw::ThreadModeRawMutex;
use portable_atomic::{AtomicU32, Ordering};
use embassy_sync::priority_channel::PriorityChannel;
use embassy_sync::pubsub::PubSubChannel;

use crate::messages::device_to_device::DeviceToDevice;

pub static THIS_SIDE_MESSAGE_BUS: PubSubChannel<ThreadModeRawMutex, DeviceToDevice, 16, 6, 6> =
    PubSubChannel::new();
pub static COMMANDS_TO_OTHER_SIDE: PriorityChannel<
    ThreadModeRawMutex,
    PrioritisedMessage<DeviceToDevice>,
    embassy_sync::priority_channel::Min,
    16,
> = PriorityChannel::new();

/// A message and its priority, lower goes first
///
/// Messages with the same priority come out in the order they went in. The
/// channel is a heap, which doesn't do that by itself, and key presses and
/// releases going out of order would lose key presses.
#[derive(Debug)]
pub struct PrioritisedMessage<T> {
    pub msg: T,
    pub priority: u8,
    seq: u32,
}

static NEXT_SEQ: AtomicU32 = AtomicU32::new(0);

impl<T> PrioritisedMessage<T> {
    pub fn new(msg: T, priority: u8) -> Self {
        Self {
            msg,
            priority,
            seq: NEXT_SEQ.fetch_add(1, Ordering::Relaxed),
        }
    }
}

impl<T> Eq for PrioritisedMessage<T> {}

impl<T> Ord for PrioritisedMessage<T> {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        // the queue is short, so comparing the difference copes with the
        // sequence number wrapping
        self.priority
            .cmp(&other.priority)
            .then((self.seq.wrapping_sub(other.seq) as i32).cmp(&0))
    }
}

impl<T> PartialEq for PrioritisedMessage<T> {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other).is_eq()
    }
}

impl<T> PartialOrd for PrioritisedMessage<T> {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
