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
