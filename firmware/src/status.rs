//! A status page for seeing how a half is doing without a probe attached, see
//! [`glove_net::status`]
//!
//! It's mostly about whether keys could be getting missed: the scanner misses
//! quick presses while it's held up, either because thread mode is too busy
//! to run it on time, or because it's waiting for room in a queue.

use core::fmt::Write;

use embassy_executor::Spawner;
use embassy_net::Stack;
use embassy_time::{Duration, Instant, Timer};
use portable_atomic::{AtomicU32, Ordering};
use shared::net::port;

/// How far behind thread mode is: timers firing late means everything that
/// runs there, the scanner included, is running late
struct Lag {
    max_us: AtomicU32,
    over_5ms: AtomicU32,
    over_50ms: AtomicU32,
}

static LAG: Lag = Lag {
    max_us: AtomicU32::new(0),
    over_5ms: AtomicU32::new(0),
    over_50ms: AtomicU32::new(0),
};

/// Times the scanner had to wait for room to pass on an event, and the
/// longest it waited
pub static SCAN_STALLS: AtomicU32 = AtomicU32::new(0);
pub static SCAN_STALL_MAX_US: AtomicU32 = AtomicU32::new(0);

#[embassy_executor::task]
async fn lag_task() -> ! {
    const PERIOD: Duration = Duration::from_millis(1);
    loop {
        let due = Instant::now() + PERIOD;
        Timer::at(due).await;

        let late = Instant::now().saturating_duration_since(due);
        LAG.max_us
            .fetch_max(late.as_micros() as u32, Ordering::Relaxed);
        if late >= Duration::from_millis(5) {
            LAG.over_5ms.fetch_add(1, Ordering::Relaxed);
        }
        if late >= Duration::from_millis(50) {
            LAG.over_50ms.fetch_add(1, Ordering::Relaxed);
        }
    }
}

#[embassy_executor::task]
async fn status_task(stack: Stack<'static>) -> ! {
    glove_net::status::serve::<2048>(stack, port::STATUS, |page| {
        _ = render(page);
    })
    .await
}

fn render(page: &mut impl Write) -> core::fmt::Result {
    let get = |c: &AtomicU32| c.load(Ordering::Relaxed);
    // maxima start over on every read, so two reads either side of a bad
    // patch show what happened in it
    let take = |c: &AtomicU32| c.swap(0, Ordering::Relaxed);

    let side = if crate::side::get_side().is_left() {
        "left"
    } else {
        "right"
    };
    writeln!(page, "{} half, up {}s", side, Instant::now().as_secs())?;
    writeln!(
        page,
        "thread lag: max {}us since last read, over 5ms {}, over 50ms {}",
        take(&LAG.max_us),
        get(&LAG.over_5ms),
        get(&LAG.over_50ms)
    )?;
    writeln!(
        page,
        "scanner stalls: {}, longest {}us since last read",
        get(&SCAN_STALLS),
        take(&SCAN_STALL_MAX_US)
    )?;
    writeln!(
        page,
        "queued: matrix {}, to other side {}, reports {}",
        crate::keys::MATRIX_EVENTS.len(),
        crate::interboard::queued(),
        crate::dongle::queued()
    )?;
    writeln!(
        page,
        "other side connected: {}",
        crate::interboard::CONNECTED.current()
    )?;
    writeln!(page, "interboard link: {:?}", crate::interboard::LINK_STATS.read())?;
    if crate::side::is_master() {
        writeln!(page, "dongle link: {:?}", crate::dongle::LINK_STATS.read())?;
    }
    writeln!(page, "radio: {:?}", glove_net::radio::stats())?;
    writeln!(page, "packet buffers ran out: {}", crate::net::pool_exhausted())?;
    writeln!(page, "longest polls since last read:")?;
    for t in crate::polls::take_worst::<8>() {
        writeln!(
            page,
            "  task {:#010x}: {}us, over 5ms {}",
            t.task, t.max_us, t.over_5ms
        )?;
    }
    Ok(())
}

pub fn init(spawner: &Spawner, stack: Stack<'static>) {
    spawner.spawn(lag_task().unwrap());
    spawner.spawn(status_task(stack).unwrap());
}
