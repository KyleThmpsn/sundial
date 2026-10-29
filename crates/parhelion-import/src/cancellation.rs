//! Cooperative cancellation scoped to one synchronous import worker.
use anyhow::{Context, Result};
use std::{
    cell::RefCell,
    fs::File,
    io::Read,
    process::{Child, ExitStatus, Output},
    sync::{
        Arc, Mutex, MutexGuard, TryLockError,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::Duration,
};

thread_local! {
    static ACTIVE: RefCell<Option<Arc<AtomicBool>>> = const { RefCell::new(None) };
}

#[derive(Debug)]
pub struct Cancelled;

impl std::fmt::Display for Cancelled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Import cancelled")
    }
}

impl std::error::Error for Cancelled {}

pub fn is_cancelled(error: &anyhow::Error) -> bool {
    error.downcast_ref::<Cancelled>().is_some()
}

pub fn check() -> Result<()> {
    if ACTIVE.with(|active| {
        active
            .borrow()
            .as_ref()
            .is_some_and(|flag| flag.load(Ordering::Relaxed))
    }) {
        return Err(Cancelled.into());
    }
    Ok(())
}

struct Scope(Option<Arc<AtomicBool>>);

impl Drop for Scope {
    fn drop(&mut self) {
        ACTIVE.with(|active| *active.borrow_mut() = self.0.take());
    }
}

/// Restores the previous scope even when conversion fails or unwinds.
pub fn run<T>(cancel: Arc<AtomicBool>, work: impl FnOnce() -> Result<T>) -> Result<T> {
    let _scope = Scope(ACTIVE.with(|active| active.replace(Some(cancel))));
    check()?;
    let result = work();
    check()?;
    result
}

pub(crate) fn lock(file: &File) -> Result<()> {
    loop {
        check()?;
        match fs2::FileExt::try_lock_exclusive(file) {
            Ok(()) => return Ok(()),
            Err(error) if error.raw_os_error() == fs2::lock_contended_error().raw_os_error() => {
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(error) => return Err(error.into()),
        }
    }
}

pub(crate) fn mutex<T>(mutex: &Mutex<T>) -> Result<MutexGuard<'_, T>> {
    loop {
        check()?;
        match mutex.try_lock() {
            Ok(guard) => return Ok(guard),
            Err(TryLockError::WouldBlock) => std::thread::sleep(Duration::from_millis(25)),
            Err(TryLockError::Poisoned(_)) => anyhow::bail!("Import cache lock is poisoned"),
        }
    }
}

/// Pure conversion batches leave capacity for the UI and game. The override also permits
/// end-to-end qualification of serial and parallel output using the same source inputs.
pub(crate) fn workers() -> Result<usize> {
    if let Some(value) = std::env::var_os("PARHELION_BUILD_WORKERS") {
        let count = value
            .to_str()
            .context("Build worker count is not UTF-8")?
            .parse::<usize>()?;
        anyhow::ensure!(
            (1..=16).contains(&count),
            "Build worker count must be between 1 and 16"
        );
        return Ok(count);
    }
    Ok((std::thread::available_parallelism().map_or(1, usize::from) / 2).clamp(1, 8))
}

/// Evaluate pure `work` over any number of `items` on [`workers`] threads and retain input order.
/// Each worker takes the next item no worker has taken, so items of uneven cost balance.
pub(crate) fn parallel_map<T: Sync, R: Send>(
    items: &[T],
    work: impl Fn(&T) -> R + Sync,
) -> Result<Vec<R>> {
    let workers = workers()?.min(items.len()).max(1);
    let lanes = (0..workers).collect::<Vec<_>>();
    let next = std::sync::atomic::AtomicUsize::new(0);
    let mut done = parallel(&lanes, |_| {
        let mut done = Vec::new();
        loop {
            check()?;
            let index = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let Some(item) = items.get(index) else {
                return Ok(done);
            };
            done.push((index, work(item)));
        }
    })?
    .into_iter()
    .flatten()
    .collect::<Vec<_>>();
    done.sort_by_key(|(index, _)| *index);
    Ok(done.into_iter().map(|(_, result)| result).collect())
}

type Job<R> = Box<dyn FnOnce() -> Result<R> + Send>;

/// Workers that evaluate pure jobs beside the caller's own serial work, with the caller's
/// cancellation scope. [`Background::finish`] returns the results in submission order, and the
/// caller commits graph mutations from them only then.
pub(crate) struct Background<R> {
    jobs: Option<mpsc::Sender<(usize, Job<R>)>>,
    results: mpsc::Receiver<(usize, Result<R>)>,
    workers: Vec<std::thread::JoinHandle<()>>,
    submitted: usize,
}

impl<R: Send + 'static> Background<R> {
    pub(crate) fn new() -> Result<Self> {
        let token = ACTIVE.with(|active| active.borrow().clone());
        let (jobs, queue) = mpsc::channel::<(usize, Job<R>)>();
        let queue = Arc::new(Mutex::new(queue));
        let (sender, results) = mpsc::channel();
        let workers = (0..workers()?)
            .map(|_| {
                let queue = queue.clone();
                let sender = sender.clone();
                let token = token.clone();
                std::thread::spawn(move || {
                    loop {
                        let job = queue.lock().ok().and_then(|queue| queue.recv().ok());
                        let Some((index, job)) = job else {
                            return;
                        };
                        let result = match &token {
                            Some(token) => run(token.clone(), job),
                            None => job(),
                        };
                        if sender.send((index, result)).is_err() {
                            return;
                        }
                    }
                })
            })
            .collect();
        Ok(Self {
            jobs: Some(jobs),
            results,
            workers,
            submitted: 0,
        })
    }

    pub(crate) fn submit(&mut self, job: impl FnOnce() -> Result<R> + Send + 'static) {
        if let Some(jobs) = &self.jobs {
            let _ = jobs.send((self.submitted, Box::new(job)));
        }
        self.submitted += 1;
    }

    /// Waits for every submitted job. The first failed job's error, in submission order, is
    /// returned.
    pub(crate) fn finish(mut self) -> Result<Vec<R>> {
        drop(self.jobs.take());
        let mut done = Vec::with_capacity(self.submitted);
        while done.len() < self.submitted {
            check()?;
            match self.results.recv_timeout(Duration::from_millis(25)) {
                Ok(result) => done.push(result),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    anyhow::bail!("Conversion worker stopped")
                }
            }
        }
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
        done.sort_by_key(|(index, _)| *index);
        done.into_iter().map(|(_, result)| result).collect()
    }
}

/// Evaluate a bounded pure batch with the caller's cancellation scope and retain input order.
/// Callers commit graph mutations only after all workers have joined successfully.
pub(crate) fn parallel<T: Sync, R: Send>(
    inputs: &[T],
    work: impl Fn(&T) -> Result<R> + Sync,
) -> Result<Vec<R>> {
    check()?;
    anyhow::ensure!(
        inputs.len() <= 16,
        "Conversion batch exceeds its worker bound"
    );
    if inputs.len() <= 1 {
        return inputs.iter().map(work).collect();
    }
    let token = ACTIVE.with(|active| active.borrow().clone());
    let result = std::thread::scope(|scope| {
        let handles = inputs
            .iter()
            .map(|input| {
                let token = token.clone();
                let work = &work;
                scope.spawn(move || match token {
                    Some(token) => run(token, || work(input)),
                    None => work(input),
                })
            })
            .collect::<Vec<_>>();
        handles
            .into_iter()
            .map(|handle| {
                handle
                    .join()
                    .map_err(|_| anyhow::anyhow!("Conversion worker stopped"))?
            })
            .collect::<Result<Vec<_>>>()
    });
    check()?;
    result
}

fn wait(child: &mut Child) -> Result<ExitStatus> {
    loop {
        if let Err(error) = check() {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) => std::thread::sleep(Duration::from_millis(25)),
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error.into());
            }
        }
    }
}

fn drain(mut pipe: Option<impl Read>) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    if let Some(pipe) = &mut pipe {
        pipe.read_to_end(&mut bytes)?;
    }
    Ok(bytes)
}

/// Drain pipes concurrently, kill the active tool on cancellation, and reap it.
/// Callers feeding stdin must first take that handle and write concurrently.
pub fn wait_with_output(mut child: Child) -> Result<Output> {
    drop(child.stdin.take());
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    std::thread::scope(|scope| {
        let stdout = scope.spawn(move || drain(stdout));
        let stderr = scope.spawn(move || drain(stderr));
        let status = wait(&mut child);
        let stdout = stdout.join();
        let stderr = stderr.join();
        Ok(Output {
            status: status?,
            stdout: stdout.map_err(|_| anyhow::anyhow!("Tool output reader stopped"))??,
            stderr: stderr.map_err(|_| anyhow::anyhow!("Tool error reader stopped"))??,
        })
    })
}

/// A native call cannot always be interrupted safely. Let owned computation or
/// read-only discovery finish privately while cancellation releases the import.
/// Work passed here must not publish files or mutate an import graph.
pub(crate) fn compute<T: Send + 'static>(
    work: impl FnOnce() -> Result<T> + Send + 'static,
) -> Result<T> {
    check()?;
    if ACTIVE.with(|active| active.borrow().is_none()) {
        return work();
    }
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = sender.send(work());
    });
    loop {
        check()?;
        match receiver.recv_timeout(Duration::from_millis(25)) {
            Ok(result) => {
                check()?;
                return result;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(error) => return Err(error).context("Import worker stopped"),
        }
    }
}
