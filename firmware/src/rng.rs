use core::{cell::RefCell, num::Wrapping};

use embassy_nrf::rng::Rng;
use embassy_sync::blocking_mutex::{raw::ThreadModeRawMutex, Mutex};
use embassy_time::Instant;
use nrf_softdevice::Softdevice;
use once_cell::sync::OnceCell;
use rand::{rngs::SmallRng, SeedableRng};

static RNG: OnceCell<Mutex<ThreadModeRawMutex, RefCell<SmallRng>>> = OnceCell::new();

pub struct MyRng;

pub fn splitmix64(x: u64) -> u64 {
    let mut z = Wrapping(x) + Wrapping(0x9E3779B97F4A7C15);
    z = (z ^ (z >> 30)) * Wrapping(0xBF58476D1CE4E5B9);
    z = (z ^ (z >> 27)) * Wrapping(0x94D049BB133111EB);
    z = z ^ (z >> 31);
    z.0
}

impl rand::RngCore for MyRng {
    fn next_u32(&mut self) -> u32 {
        RNG.get().unwrap().lock(|r| r.borrow_mut().next_u32())
    }

    fn next_u64(&mut self) -> u64 {
        RNG.get().unwrap().lock(|r| r.borrow_mut().next_u64())
    }

    fn fill_bytes(&mut self, dest: &mut [u8]) {
        RNG.get().unwrap().lock(|r| r.borrow_mut().fill_bytes(dest))
    }

    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), rand::Error> {
        RNG.get()
            .unwrap()
            .lock(|r| r.borrow_mut().try_fill_bytes(dest))
    }
}

pub async fn init(sd: &Softdevice) {
    let mut buf = [0u8; 8];
    while let Err(_) = nrf_softdevice::random_bytes(sd, &mut buf) {
        embassy_time::Timer::after_millis(10).await;
    }
    crate::log::debug!("Got random bytes: {=[u8; 8]}", buf);
    let mut seed = splitmix64(u64::from_be_bytes(buf));
    seed ^= splitmix64(Instant::now().as_ticks());

    let _ = RNG.set(Mutex::new(RefCell::new(SmallRng::seed_from_u64(seed))));
}
