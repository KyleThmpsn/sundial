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
