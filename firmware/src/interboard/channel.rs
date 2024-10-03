use embassy_sync::blocking_mutex::raw::ThreadModeRawMutex;
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

#[derive(Debug)]
pub struct PrioritisedMessage<T> {
    pub msg: T,
    pub priority: u8,
}

impl<T> Eq for PrioritisedMessage<T> {}

impl<T> Ord for PrioritisedMessage<T> {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.priority.cmp(&other.priority)
    }
}

impl<T> PartialEq for PrioritisedMessage<T> {
    fn eq(&self, other: &Self) -> bool {
        self.priority == other.priority
    }
}

impl<T> PartialOrd for PrioritisedMessage<T> {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        self.priority.partial_cmp(&other.priority)
    }
}
