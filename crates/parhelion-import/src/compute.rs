//! Bounded hardware compute for offline conversion, with an observable CPU fallback.
use anyhow::{Result, bail};
use serde::Serialize;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

#[cfg(windows)]
mod windows;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Auto,
    Cpu,
    Gpu,
}

fn mode() -> Result<Mode> {
    match std::env::var("PARHELION_COMPUTE").as_deref() {
        Ok("auto") | Err(std::env::VarError::NotPresent) => Ok(Mode::Auto),
        Ok("cpu") => Ok(Mode::Cpu),
        Ok("gpu") => Ok(Mode::Gpu),
        _ => bail!("PARHELION_COMPUTE must be auto, cpu or gpu"),
    }
}

/// A checked, tightly packed structured input buffer. Only the Windows device reads it.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) struct Input<'a> {
    pub bytes: &'a [u8],
    pub stride: usize,
}

/// Completed hardware work only. These counters do not count shader compilation as execution.
#[derive(Clone, Default, Serialize)]
pub struct Snapshot {
    pub mode: String,
    pub dispatches: u64,
    pub elements: u64,
    pub seconds: f64,
    pub adapter: Option<String>,
    pub fallback: Option<String>,
}

fn counters() -> &'static Mutex<Snapshot> {
    static COUNTERS: OnceLock<Mutex<Snapshot>> = OnceLock::new();
    COUNTERS.get_or_init(Mutex::default)
}

/// Build diagnostics can take a delta around one conversion without retaining GPU resources.
pub fn snapshot() -> Snapshot {
    counters().lock().unwrap_or_else(|e| e.into_inner()).clone()
}

impl Snapshot {
    pub fn since(&self, earlier: &Self) -> Self {
        Self {
            mode: std::env::var("PARHELION_COMPUTE").unwrap_or_else(|_| "auto".into()),
            dispatches: self.dispatches.saturating_sub(earlier.dispatches),
            elements: self.elements.saturating_sub(earlier.elements),
            seconds: (self.seconds - earlier.seconds).max(0.),
            adapter: self.adapter.clone(),
            fallback: self.fallback.clone(),
        }
    }
}

/// Small jobs stay on the CPU unless hardware execution is explicitly required for verification.
pub(crate) fn eligible(elements: usize) -> Result<bool> {
    Ok(match mode()? {
        Mode::Cpu => false,
        Mode::Gpu => true,
        Mode::Auto => elements >= 65_536,
    })
}

/// Reject malformed hardware output with the same policy as a failed device operation.
/// Source validation belongs before this boundary and must never become a fallback.
pub(crate) fn checked<T>(result: Result<Option<T>>) -> Result<Option<T>> {
    match result {
        Err(error) if !crate::cancellation::is_cancelled(&error) && mode()? != Mode::Gpu => {
            counters()
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .fallback = Some(format!("{error:#}"));
            Ok(None)
        }
        result => result,
    }
}

/// Run one bounded dispatch and return its fully read-back bytes. A failed hardware operation
/// leaves no partial result. `auto` falls back, while `gpu` makes qualification failures explicit.
pub(crate) fn dispatch(
    source: &'static str,
    constants: [u32; 4],
    inputs: &[Input<'_>],
    stride: usize,
    elements: usize,
) -> Result<Option<Vec<u8>>> {
    crate::cancellation::check()?;
    let selected = mode()?;
    if selected == Mode::Cpu {
        counters()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .fallback = Some("CPU execution was selected".into());
        return Ok(None);
    }
    let started = Instant::now();
    #[cfg(windows)]
    let result = windows::dispatch(source, constants, inputs, stride, elements);
    #[cfg(not(windows))]
    let result: Result<(Vec<u8>, String)> = {
        let _ = (source, constants, inputs, stride, elements);
        Err(anyhow::anyhow!(
            "Hardware conversion requires Direct3D 11 on Windows"
        ))
    };
    match result {
        Ok((bytes, adapter)) => {
            crate::cancellation::check()?;
            let mut counters = counters().lock().unwrap_or_else(|e| e.into_inner());
            counters.dispatches += 1;
            counters.elements += elements as u64;
            counters.seconds += started.elapsed().as_secs_f64();
            counters.adapter = Some(adapter);
            Ok(Some(bytes))
        }
        Err(error) => {
            if crate::cancellation::is_cancelled(&error) || selected == Mode::Gpu {
                return Err(error);
            }
            counters()
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .fallback = Some(format!("{error:#}"));
            Ok(None)
        }
    }
}
