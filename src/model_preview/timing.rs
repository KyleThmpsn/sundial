//! Opt-in stage timing for preview loads and the paint thread's GPU preparation.
//!
//! `SUNDIAL_PREVIEW_TIMING` names a file that receives one JSON line per timed load and per
//! paint-thread step longer than a millisecond. A step's stages exclude the stages nested in them,
//! so they add up to the step's total. With timing off, a stage costs one atomic load.
use serde::Serialize;
use std::{
    cell::RefCell,
    io::Write,
    path::PathBuf,
    sync::{
        Mutex, OnceLock, PoisonError,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

static RECORDING: AtomicBool = AtomicBool::new(false);
static EVENTS: Mutex<Vec<Event>> = Mutex::new(Vec::new());

/// One timed step: a model load on its worker, or GPU work on the paint thread.
#[derive(Clone, Debug, Serialize)]
pub struct Event {
    pub name: String,
    pub thread: &'static str,
    /// When the step began, in milliseconds since timing started.
    pub start_ms: f64,
    pub total_ms: f64,
    /// Each stage's own milliseconds and how often it ran.
    pub stages: Vec<(&'static str, f64, u32)>,
}

struct Open {
    name: &'static str,
    start: Instant,
    nested: Duration,
}

thread_local! {
    static OPEN: RefCell<Option<Vec<Open>>> = const { RefCell::new(None) };
    static TOTALS: RefCell<Vec<(&'static str, Duration, u32)>> = const { RefCell::new(Vec::new()) };
}

fn epoch() -> Instant {
    static EPOCH: OnceLock<Instant> = OnceLock::new();
    *EPOCH.get_or_init(Instant::now)
}

fn output() -> Option<&'static PathBuf> {
    static OUTPUT: OnceLock<Option<PathBuf>> = OnceLock::new();
    OUTPUT
        .get_or_init(|| std::env::var_os("SUNDIAL_PREVIEW_TIMING").map(PathBuf::from))
        .as_ref()
}

fn enabled() -> bool {
    RECORDING.load(Ordering::Relaxed) || output().is_some()
}

/// Keeps events in memory for [`take`], as a timing run does.
#[cfg(feature = "test-support")]
pub fn record(on: bool) {
    epoch();
    RECORDING.store(on, Ordering::Relaxed);
}

/// The events recorded since the last call.
#[cfg(feature = "test-support")]
pub fn take() -> Vec<Event> {
    std::mem::take(&mut *EVENTS.lock().unwrap_or_else(PoisonError::into_inner))
}

/// Milliseconds since timing started, on the clock events use.
#[cfg(feature = "test-support")]
pub fn now_ms() -> f64 {
    millis(epoch().elapsed())
}

static SALT: OnceLock<String> = OnceLock::new();

/// Makes this process's preview shaders differ from any a driver has cached, so a timing run
/// measures a first compile.
#[cfg(feature = "test-support")]
pub fn salt_shaders(salt: String) {
    let _ = SALT.set(salt);
}

/// A comment to append to preview shader sources, set only by a timing run.
pub(crate) fn shader_salt() -> Option<&'static str> {
    SALT.get().map(String::as_str)
}

/// Records elapsed time for a named stage while a timing step is active.
#[must_use]
pub struct Stage(bool);

pub fn stage(name: &'static str) -> Stage {
    if !enabled() {
        return Stage(false);
    }
    Stage(OPEN.with(|open| {
        open.borrow_mut().as_mut().is_some_and(|open| {
            open.push(Open {
                name,
                start: Instant::now(),
                nested: Duration::ZERO,
            });
            true
        })
    }))
}

impl Drop for Stage {
    fn drop(&mut self) {
        if !self.0 {
            return;
        }
        OPEN.with(|open| {
            let mut open = open.borrow_mut();
            let Some(open) = open.as_mut() else {
                return;
            };
            let Some(closed) = open.pop() else {
                return;
            };
            let elapsed = closed.start.elapsed();
            if let Some(parent) = open.last_mut() {
                parent.nested += elapsed;
            }
            let own = elapsed.saturating_sub(closed.nested);
            TOTALS.with(|totals| {
                let mut totals = totals.borrow_mut();
                match totals.iter_mut().find(|(name, ..)| *name == closed.name) {
                    Some(total) => {
                        total.1 += own;
                        total.2 += 1;
                    }
                    None => totals.push((closed.name, own, 1)),
                }
            });
        });
    }
}

/// Runs `work` as one step named `name`, with time outside any stage counted toward `rest`.
/// The step is published when it took at least `minimum`.
pub(crate) fn measure<T>(
    name: impl FnOnce() -> String,
    thread: &'static str,
    rest: &'static str,
    minimum: Duration,
    work: impl FnOnce() -> T,
) -> T {
    let nested = OPEN.with(|open| open.borrow().is_some());
    if !enabled() || nested {
        return work();
    }
    OPEN.with(|open| *open.borrow_mut() = Some(Vec::new()));
    let epoch = epoch();
    let start = Instant::now();
    let result = {
        let _rest = stage(rest);
        work()
    };
    let total = start.elapsed();
    OPEN.with(|open| *open.borrow_mut() = None);
    let stages = TOTALS.with(|totals| std::mem::take(&mut *totals.borrow_mut()));
    if total >= minimum {
        publish(Event {
            name: name(),
            thread,
            start_ms: millis(start.saturating_duration_since(epoch)),
            total_ms: millis(total),
            stages: stages
                .into_iter()
                .map(|(stage, time, count)| (stage, millis(time), count))
                .collect(),
        });
    }
    result
}

/// Records that `name` happened now, such as a model's first drawn frame.
pub(crate) fn mark(name: impl FnOnce() -> String, thread: &'static str) {
    if !enabled() {
        return;
    }
    publish(Event {
        name: name(),
        thread,
        start_ms: millis(epoch().elapsed()),
        total_ms: 0.0,
        stages: Vec::new(),
    });
}

fn millis(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

fn publish(event: Event) {
    if let Some(path) = output()
        && let Ok(line) = serde_json::to_string(&event)
        && let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
    {
        let _ = writeln!(file, "{line}");
    }
    if RECORDING.load(Ordering::Relaxed) {
        EVENTS
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(event);
    }
}
