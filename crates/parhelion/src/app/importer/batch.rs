//! Bounded conversion workers with serial publication and explicit unfinished results.
use std::{
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc,
    },
    thread,
};

#[cfg(test)]
mod tests;

#[derive(Default)]
pub(super) struct Report {
    pub completed: usize,
    pub cancelled: bool,
}

pub(super) fn run<R: Send>(
    total: usize,
    workers: usize,
    cancel: &AtomicBool,
    convert: impl Fn(usize, usize) -> Result<R, String> + Sync,
    mut accept: impl FnMut(usize, Result<R, String>),
) -> Report {
    if total == 0 {
        return Report::default();
    }
    let workers = workers.clamp(1, total);
    let next = AtomicUsize::new(0);
    let started: Vec<_> = (0..total).map(|_| AtomicBool::new(false)).collect();
    let mut answered = vec![false; total];
    let mut completed = 0;
    thread::scope(|scope| {
        let (sender, results) = mpsc::sync_channel(workers);
        let handles: Vec<_> = (0..workers)
            .map(|slot| {
                let sender = sender.clone();
                let (next, started, convert) = (&next, &started, &convert);
                scope.spawn(move || convert_items(slot, next, started, cancel, sender, convert))
            })
            .collect();
        drop(sender);
        for (index, result) in results {
            answered[index] = true;
            completed += 1;
            accept(index, result);
        }
        // Handle panics here so the caller can retain and report completed saves.
        for handle in handles {
            let _ = handle.join();
        }
    });
    let cancelled = cancel.load(Ordering::Relaxed);
    for (index, answered) in answered.into_iter().enumerate() {
        if !answered && (!cancelled || started[index].load(Ordering::Relaxed)) {
            completed += 1;
            accept(
                index,
                Err("An import worker stopped before returning a result. Retry this item.".into()),
            );
        }
    }
    Report {
        completed,
        cancelled,
    }
}

fn convert_items<R>(
    slot: usize,
    next: &AtomicUsize,
    started: &[AtomicBool],
    cancel: &AtomicBool,
    sender: mpsc::SyncSender<(usize, Result<R, String>)>,
    convert: &impl Fn(usize, usize) -> Result<R, String>,
) {
    while !cancel.load(Ordering::Relaxed) {
        let index = next.fetch_add(1, Ordering::Relaxed);
        let Some(started) = started.get(index) else {
            return;
        };
        started.store(true, Ordering::Relaxed);
        let result = convert(slot, index);
        if sender.send((index, result)).is_err() {
            return;
        }
    }
}
