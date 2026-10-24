//! How long each task's polls take, from the executor's trace hooks. A task
//! that takes long to poll holds up everything else on its executor, the key
//! scanner included. See [`crate::status`].
//!
//! Tasks are known by the address of their header, which lies within their
//! task pool's static, so `tools/glove-status.py` can name them from the ELF.

use core::cell::RefCell;

use embassy_executor::{
    raw::{trace::Trace, TaskRef},
    ExecutorId,
};
use embassy_sync::blocking_mutex::{raw::CriticalSectionRawMutex, Mutex};
use embassy_time::{Duration, Instant};

const TASKS: usize = 48;
/// The thread mode executor, and the radio's interrupt executor
const EXECUTORS: usize = 2;

#[derive(Clone, Copy, Default)]
pub struct TaskPolls {
    pub task: usize,
    /// The longest poll since [`take_worst`]
    pub max_us: u32,
    pub over_5ms: u32,
}

struct Polls {
    tasks: heapless::Vec<TaskPolls, TASKS>,
    /// What each executor is polling: (executor, task, started)
    running: [Option<(usize, usize, Instant)>; EXECUTORS],
}

static POLLS: Mutex<CriticalSectionRawMutex, RefCell<Polls>> = Mutex::new(RefCell::new(Polls {
    tasks: heapless::Vec::new(),
    running: [None; EXECUTORS],
}));

fn task_id(task: TaskRef) -> usize {
    task.as_raw().as_ptr() as usize
}

struct PollTimer;

impl Trace for PollTimer {
    fn poll_start(_executor: ExecutorId) {}
    fn task_new(_executor: ExecutorId, _task: TaskRef) {}
    fn task_end(_executor: ExecutorId, _task: TaskRef) {}

    fn task_exec_begin(executor: ExecutorId, task: TaskRef) {
        let now = Instant::now();
        POLLS.lock(|p| {
            let mut p = p.borrow_mut();
            let executor = executor.get();
            let slot = p
                .running
                .iter()
                .position(|r| matches!(r, Some((e, _, _)) if *e == executor))
                .or_else(|| p.running.iter().position(|r| r.is_none()));
            if let Some(slot) = slot {
                p.running[slot] = Some((executor, task_id(task), now));
            }
        });
    }

    fn task_exec_end(executor: ExecutorId, task: TaskRef) {
        let now = Instant::now();
        POLLS.lock(|p| {
            let mut p = p.borrow_mut();
            let executor = executor.get();
            let Some(slot) = p
                .running
                .iter()
                .position(|r| matches!(r, Some((e, _, _)) if *e == executor))
            else {
                return;
            };
            let Some((_, id, started)) = p.running[slot].take() else {
                return;
            };
            if id != task_id(task) {
                return;
            }

            let took = now.saturating_duration_since(started);
            let entry = match p.tasks.iter().position(|t| t.task == id) {
                Some(i) => &mut p.tasks[i],
                None => {
                    if p.tasks
                        .push(TaskPolls {
                            task: id,
                            ..Default::default()
                        })
                        .is_err()
                    {
                        return;
                    }
                    p.tasks.last_mut().unwrap()
                }
            };
            entry.max_us = entry.max_us.max(took.as_micros() as u32);
            if took >= Duration::from_millis(5) {
                entry.over_5ms += 1;
            }
        });
    }

    fn task_ready_begin(_executor: ExecutorId, _task: TaskRef) {}
    fn executor_idle(_executor: ExecutorId) {}
    fn idle() {}
    fn task_name_set(_task: TaskRef, _name: &'static str) {}
    fn task_priority_set(_task: TaskRef, _priority: u8) {}
    fn task_deadline_set(_task: TaskRef, _deadline: u64) {}
}

embassy_executor::trace_impl!(PollTimer);

/// The `N` tasks with the longest polls since last time, longest first.
/// Starts the longest polls over.
pub fn take_worst<const N: usize>() -> heapless::Vec<TaskPolls, N> {
    POLLS.lock(|p| {
        let mut p = p.borrow_mut();
        let mut worst = heapless::Vec::<TaskPolls, N>::new();
        for t in p.tasks.iter() {
            let at = worst
                .iter()
                .position(|w| t.max_us > w.max_us)
                .unwrap_or(worst.len());
            if at < N {
                if worst.is_full() {
                    worst.pop();
                }
                _ = worst.insert(at, *t);
            }
        }
        for t in p.tasks.iter_mut() {
            t.max_us = 0;
        }
        worst
    })
}
