use core::cell::Cell;

use embassy_sync::blocking_mutex::raw::{RawMutex, ThreadModeRawMutex};
use maitake_sync::blocking::{ConstInit, ScopedRawMutex};

pub struct ThreadModeScopedMutex(ThreadModeRawMutex);

impl ConstInit for ThreadModeScopedMutex {
    const INIT: Self = ThreadModeScopedMutex(ThreadModeRawMutex::new());
}

unsafe impl ScopedRawMutex for ThreadModeScopedMutex {
    fn try_with_lock<R>(&self, f: impl FnOnce() -> R) -> Option<R> {
        // it's impossible to lock this mutex twice
        //
        // reentrancy probably breaks it, but the consumer of this mutex (mycelium) shouldn't do that
        Some(self.0.lock(f))
    }

    fn with_lock<R>(&self, f: impl FnOnce() -> R) -> R {
        self.0.lock(f)
    }

    fn is_locked(&self) -> bool {
        false
    }
}

pub type Mutex<T> = maitake_sync::Mutex<T, ThreadModeScopedMutex>;
pub type RwLock<T> = maitake_sync::RwLock<T, ThreadModeScopedMutex>;
pub type WaitCell = maitake_sync::WaitCell;
pub type WaitMap<K, V> = maitake_sync::WaitMap<K, V, ThreadModeScopedMutex>;
pub type WaitQueue = maitake_sync::WaitQueue<ThreadModeScopedMutex>;

pub const fn mutex<T>(data: T) -> Mutex<T> {
    Mutex::new_with_raw_mutex(data, ThreadModeScopedMutex::INIT)
}

/// Something that stores a current value and allows receivers to wait for it to
/// change.
///
/// This doesn't ensure every reader sees every change, only that each reader is
/// woken when a change occurs
pub struct Watch<T: Clone> {
    data: maitake_sync::blocking::Mutex<T, ThreadModeScopedMutex>,
    queue: WaitQueue,
}

impl<T: Clone> Watch<T> {
    pub const fn new(initial: T) -> Self {
        Self {
            data: maitake_sync::blocking::Mutex::new_with_raw_mutex(
                initial,
                ThreadModeScopedMutex::INIT,
            ),
            queue: WaitQueue::new_with_raw_mutex(ThreadModeScopedMutex::INIT),
        }
    }

    pub fn current(&self) -> T {
        self.data.with_lock(|x| x.clone())
    }

    pub async fn wait(&self) -> T {
        self.queue.wait().await.unwrap();
        self.current()
    }

    pub async fn wait_for(&self, mut f: impl FnMut(&T) -> bool) -> T {
        self.queue
            .wait_for_value(|| {
                let v = self.current();
                f(&v).then_some(v)
            })
            .await
            .unwrap()
    }

    pub fn set(&self, new: T) {
        self.data.with_lock(|x| *x = new);
        self.queue.wake_all();
    }
}
