//! A packet buffer pool that counts the times it ran out

use embassy_net::Pool;
use portable_atomic::{AtomicU32, Ordering};
use xarxa::driver::PacketBuf;

pub struct CountingPool<P> {
    inner: P,
    exhausted: AtomicU32,
}

impl<P> CountingPool<P> {
    pub const fn new(inner: P) -> Self {
        Self {
            inner,
            exhausted: AtomicU32::new(0),
        }
    }

    /// How many times a buffer was wanted and there wasn't one
    pub fn exhausted(&self) -> u32 {
        self.exhausted.load(Ordering::Relaxed)
    }
}

impl<P: Pool> Pool for CountingPool<P> {
    fn alloc(&'static self) -> Option<PacketBuf> {
        let buf = self.inner.alloc();
        if buf.is_none() {
            self.exhausted.fetch_add(1, Ordering::Relaxed);
        }
        buf
    }

    fn buf_capacity(&self) -> usize {
        self.inner.buf_capacity()
    }
}
