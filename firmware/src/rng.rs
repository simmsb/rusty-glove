use core::{cell::RefCell, convert::Infallible, num::Wrapping};

use embassy_nrf::mode::Async;
use embassy_sync::blocking_mutex::{raw::ThreadModeRawMutex, Mutex};
use embassy_time::Instant;
use once_cell::sync::OnceCell;
use rand::{rngs::SmallRng, Rng, SeedableRng};

static RNG: OnceCell<Mutex<ThreadModeRawMutex, RefCell<SmallRng>>> = OnceCell::new();

pub struct MyRng;

pub fn splitmix64(x: u64) -> u64 {
    let mut z = Wrapping(x) + Wrapping(0x9E3779B97F4A7C15);
    z = (z ^ (z >> 30)) * Wrapping(0xBF58476D1CE4E5B9);
    z = (z ^ (z >> 27)) * Wrapping(0x94D049BB133111EB);
    z = z ^ (z >> 31);
    z.0
}

impl rand::TryRng for MyRng {
    type Error = Infallible;

    fn try_next_u32(&mut self) -> Result<u32, Infallible> {
        Ok(RNG.get().unwrap().lock(|r| r.borrow_mut().next_u32()))
    }

    fn try_next_u64(&mut self) -> Result<u64, Infallible> {
        Ok(RNG.get().unwrap().lock(|r| r.borrow_mut().next_u64()))
    }

    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), Infallible> {
        RNG.get().unwrap().lock(|r| r.borrow_mut().fill_bytes(dest));
        Ok(())
    }
}

/// Seeds our RNG from the hardware one, returning another seed for the network stack
pub async fn init(rng: &mut embassy_nrf::rng::Rng<'_, Async>) -> u64 {
    crate::log::info!("Initialising the RNG");
    let mut buf = [0u8; 8];
    rng.fill_bytes(&mut buf).await;
    crate::log::debug!("Got random bytes: {=[u8; 8]}", buf);
    let mut seed = splitmix64(u64::from_be_bytes(buf));
    seed ^= splitmix64(Instant::now().as_ticks());

    let _ = RNG.set(Mutex::new(RefCell::new(SmallRng::seed_from_u64(seed))));
    crate::log::info!("RNG initialised");

    rng.fill_bytes(&mut buf).await;
    u64::from_le_bytes(buf)
}
