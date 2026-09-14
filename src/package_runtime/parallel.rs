//! Runs independent jobs on a small pool of worker threads. Every package has its own
//! reader lock inside the manager, so one job per package keeps the readers busy without
//! contending for the same file.
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    mpsc,
};

/// How many worker threads a job list deserves: the machine's parallelism, capped so a
/// scan does not starve the game or the editor, and never more than there are jobs.
pub(crate) fn worker_count(jobs: usize) -> usize {
    std::thread::available_parallelism()
        .map_or(4, std::num::NonZeroUsize::get)
        .clamp(1, 8)
        .min(jobs)
}

/// Applies `work` to every job on the pool and returns the results in job order.
pub(crate) fn map_jobs<J: Sync, T: Send>(jobs: &[J], work: impl Fn(&J) -> T + Sync) -> Vec<T> {
    let mut results = jobs.iter().map(|_| None).collect::<Vec<Option<T>>>();
    if jobs.is_empty() {
        return Vec::new();
    }
    let next = AtomicUsize::new(0);
    let (sender, receiver) = mpsc::channel();
    std::thread::scope(|scope| {
        for _ in 0..worker_count(jobs.len()) {
            let sender = sender.clone();
            let next = &next;
            let work = &work;
            scope.spawn(move || {
                loop {
                    let job = next.fetch_add(1, Ordering::Relaxed);
                    let Some(input) = jobs.get(job) else {
                        break;
                    };
                    if sender.send((job, work(input))).is_err() {
                        break;
                    }
                }
            });
        }
        drop(sender);
        for (job, result) in receiver {
            results[job] = Some(result);
        }
    });
    results
        .into_iter()
        .map(|result| result.expect("every job reports a result"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn results_come_back_in_job_order_whatever_the_workers_do() {
        let jobs = (0..100_u64).collect::<Vec<_>>();
        let results = map_jobs(&jobs, |job| {
            // Later jobs finish first, so ordering must come from the job index.
            std::thread::sleep(std::time::Duration::from_micros(200 - job * 2));
            job * job
        });
        assert_eq!(
            results,
            jobs.iter().map(|job| job * job).collect::<Vec<_>>()
        );
        assert!(map_jobs(&Vec::<u8>::new(), |_| 1).is_empty());
    }
}
