#![allow(unused)]

use core::{arch::asm, marker::PhantomData};

#[cfg(feature = "probe")]
pub use defmt as log;
use embassy_executor::Spawner;
use embassy_time::{Duration, Instant, Timer};

#[cfg(feature = "logging")]
pub use log_log as log;

#[cfg(feature = "logging")]
pub trait WhichDebug = ::core::fmt::Debug;
#[cfg(feature = "probe")]
pub trait WhichDebug = ::defmt::Format;

#[cfg(not(any(feature = "logging", feature = "probe")))]
pub trait WhichDebug = ::core::marker::Sized;

pub mod noop_log {
    macro_rules! info {
        ($($x:tt)*) => {};
    }
    macro_rules! debug {
        ($($x:tt)*) => {};
    }
    macro_rules! trace {
        ($($x:tt)*) => {};
    }

    macro_rules! error {
        ($($x:tt)*) => {};
    }
}

#[cfg(not(any(feature = "logging", feature = "probe")))]
pub use noop_log as log;

#[macro_export]
macro_rules! singleton {
    ($t:ty, $val:expr) => {{
        static STATIC_CELL: ::static_cell::StaticCell<($t,)> = ::static_cell::StaticCell::new();
        let (x,) = STATIC_CELL.init(($val,));
        x
    }};
}

#[allow(unused_imports)]
pub(crate) use singleton;

pub struct Ticker {
    last_tick: Instant,
    duration: Duration,
}

impl Ticker {
    pub fn every(duration: Duration) -> Self {
        let last_tick = Instant::now();
        Self {
            last_tick,
            duration,
        }
    }

    pub async fn next(&mut self) {
        let now = Instant::now();

        if now.saturating_duration_since(self.last_tick) > self.duration {
            self.last_tick = now;
            return;
        }

        let next_tick = self.last_tick + self.duration;

        Timer::at(next_tick).await;

        self.last_tick = next_tick;
    }
}

pub mod executor_metrics {
    use portable_atomic::AtomicU64;

    // pub static WAKEUPS: AtomicUsize = AtomicUsize::new(0);
    pub static AWAKE: AtomicU64 = AtomicU64::new(0);
    pub static SLEEP: AtomicU64 = AtomicU64::new(0);
}

pub struct MeasuringExecutor {
    inner: embassy_executor::raw::Executor,
    not_send: PhantomData<*mut ()>,
    samples: heapless::HistoryBuffer<(u16, u16), 8>,
}

const THREAD_PENDER: usize = usize::MAX;

impl Default for MeasuringExecutor {
    fn default() -> Self {
        Self::new()
    }
}

impl MeasuringExecutor {
    pub fn new() -> Self {
        Self {
            inner: embassy_executor::raw::Executor::new(THREAD_PENDER as *mut ()),
            not_send: PhantomData,
            samples: heapless::HistoryBuffer::new(),
        }
    }

    pub fn run(&'static mut self, init: impl FnOnce(Spawner)) -> ! {
        init(self.inner.spawner());

        loop {
            let start = embassy_time::Instant::now();

            unsafe {
                self.inner.poll();
            }

            let finished = embassy_time::Instant::now();

            unsafe {
                asm!("wfe");
            }

            let now = embassy_time::Instant::now();

            let awake = finished.as_ticks().saturating_sub(start.as_ticks()) as u16;
            let sleeping = now.as_ticks().saturating_sub(finished.as_ticks()) as u16;

            self.samples.write((awake, sleeping));

            let (awake, sleeping) = self.samples.iter().fold(
                (0, 0),
                |(total_awake, total_asleep), (awake, sleeping)| {
                    (total_awake + *awake as u64, total_asleep + *sleeping as u64)
                },
            );

            // executor_metrics::WAKEUPS.add(1, portable_atomic::Ordering::Relaxed);
            executor_metrics::AWAKE.add(awake, portable_atomic::Ordering::Relaxed);
            executor_metrics::SLEEP.add(sleeping, portable_atomic::Ordering::Release);
        }
    }
}

pub fn reboot_to_bootloader() -> ! {
    unsafe {
        nrf_softdevice_s140::sd_power_gpregret_clr(0, u32::MAX);
        nrf_softdevice_s140::sd_power_gpregret_set(0, 0x57);
    }
    unsafe {
        let p = embassy_nrf::pac::Peripherals::steal();

        p.POWER.gpregret.write(|w| w.gpregret().bits(0x57));

        let mut cfg: embassy_nrf::wdt::Config = Default::default();
        cfg.timeout_ticks = 1000;
        _ = embassy_nrf::wdt::Watchdog::try_new::<1>(embassy_nrf::peripherals::WDT::steal(), cfg)
            .ok()
            .unwrap();
    }

    loop {
        cortex_m::asm::nop();
    }

    // 0x57 seems to be the magic to enter UF2 DFU
    // cortex_m::peripheral::SCB::sys_reset();
}

#[allow(unused)]
pub fn debug_interrupts() {
    {
        const INTERRUPTS: &[Interrupt] = {
            use embassy_nrf::interrupt::Interrupt::*;

            &[
                POWER_CLOCK,
                RADIO,
                UARTE0_UART0,
                SPIM0_SPIS0_TWIM0_TWIS0_SPI0_TWI0,
                SPIM1_SPIS1_TWIM1_TWIS1_SPI1_TWI1,
                NFCT,
                GPIOTE,
                SAADC,
                TIMER0,
                TIMER1,
                TIMER2,
                RTC0,
                TEMP,
                RNG,
                ECB,
                CCM_AAR,
                WDT,
                RTC1,
                QDEC,
                COMP_LPCOMP,
                SWI0_EGU0,
                SWI1_EGU1,
                SWI2_EGU2,
                SWI3_EGU3,
                SWI4_EGU4,
                SWI5_EGU5,
                TIMER3,
                TIMER4,
                PWM0,
                PDM,
                MWU,
                PWM1,
                PWM2,
                SPIM2_SPIS2_SPI2,
                RTC2,
                I2S,
                FPU,
                USBD,
                UARTE1,
                QSPI,
                CRYPTOCELL,
                PWM3,
                SPIM3,
            ]
        };

        use embassy_nrf::interrupt::{Interrupt, InterruptExt};
        for interrupt in INTERRUPTS {
            let is_enabled = InterruptExt::is_enabled(*interrupt);
            let priority = InterruptExt::get_priority(*interrupt);

            if is_enabled {
                crate::log::info!(
                    "Interrupt {}: Enabled = {}, Priority = {}",
                    defmt::Debug2Format(interrupt),
                    is_enabled,
                    priority
                );
            }
        }
    }
}
