//! Bounded memory reuse with compiler-identified, validated disk generations.
use anyhow::Result;
use sha2::{Digest, Sha256};
use std::time::Instant;
use std::{
    collections::BTreeMap,
    sync::{Mutex, OnceLock},
};

#[cfg(windows)]
mod disk;

pub(super) fn compiler_identity() -> Option<String> {
    #[cfg(windows)]
    {
        disk::compiler().map(hex::encode)
    }
    #[cfg(not(windows))]
    {
        None
    }
}

#[derive(Clone, Default, serde::Serialize)]
pub struct Snapshot {
    pub memory_hits: u64,
    pub disk_hits: u64,
    pub compilations: u64,
    pub compile_seconds: f64,
}

fn counters() -> &'static Mutex<Snapshot> {
    static COUNTERS: OnceLock<Mutex<Snapshot>> = OnceLock::new();
    COUNTERS.get_or_init(Mutex::default)
}

pub(super) fn snapshot() -> Snapshot {
    counters().lock().unwrap_or_else(|e| e.into_inner()).clone()
}

impl Snapshot {
    pub fn since(&self, before: &Self) -> Self {
        Self {
            memory_hits: self.memory_hits.saturating_sub(before.memory_hits),
            disk_hits: self.disk_hits.saturating_sub(before.disk_hits),
            compilations: self.compilations.saturating_sub(before.compilations),
            compile_seconds: (self.compile_seconds - before.compile_seconds).max(0.),
        }
    }
}

type Compiled = (Vec<u8>, String);
const MAX_BYTES: usize = 32 * 1024 * 1024;
#[derive(Default)]
struct Cache {
    entries: BTreeMap<[u8; 32], Compiled>,
    access: BTreeMap<[u8; 32], u64>,
    clock: u64,
    bytes: usize,
}
impl Cache {
    fn insert(&mut self, key: [u8; 32], value: Compiled) {
        let size = value.0.len() + value.1.len();
        if size > MAX_BYTES || self.entries.contains_key(&key) {
            return;
        }
        while self.bytes + size > MAX_BYTES {
            let Some(oldest) = self
                .access
                .iter()
                .min_by_key(|(_, used)| *used)
                .map(|(key, _)| *key)
            else {
                break;
            };
            self.access.remove(&oldest);
            if let Some(value) = self.entries.remove(&oldest) {
                self.bytes -= value.0.len() + value.1.len();
            }
        }
        self.bytes += size;
        self.clock += 1;
        self.access.insert(key, self.clock);
        self.entries.insert(key, value);
    }

    fn get(&mut self, key: &[u8; 32]) -> Option<&Compiled> {
        let value = self.entries.get(key)?;
        self.clock += 1;
        self.access.insert(*key, self.clock);
        Some(value)
    }
}
fn program_key(text: &str, stage: u8) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update([stage]);
    hash.update(text.as_bytes());
    hash.finalize().into()
}
pub(super) fn compile(text: &str, vertex: bool) -> Result<Compiled> {
    cached(text, u8::from(vertex))
}

fn cached(text: &str, stage: u8) -> Result<Compiled> {
    crate::cancellation::check()?;
    static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
    let cache = CACHE.get_or_init(Mutex::default);
    let key = program_key(text, stage);
    if let Ok(mut cache) = cache.lock()
        && let Some(value) = cache.get(&key)
    {
        counters()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .memory_hits += 1;
        return Ok(value.clone());
    }
    #[cfg(windows)]
    let disk = match disk::Entry::open(&key, stage) {
        Ok(disk) => disk,
        Err(error) if crate::cancellation::is_cancelled(&error) => return Err(error),
        Err(_) => None,
    };
    #[cfg(windows)]
    if let Some(value) = disk.as_ref().and_then(|disk| disk.read()) {
        counters()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .disk_hits += 1;
        if let Ok(mut cache) = cache.lock() {
            cache.insert(key, value.clone());
        }
        return Ok(value);
    }
    // Do not hold a global lock during compilation. Errors are never cached.
    let text = text.to_owned();
    let started = Instant::now();
    let value = crate::cancellation::compute(move || compile_stage(&text, stage))?;
    {
        let mut count = counters().lock().unwrap_or_else(|e| e.into_inner());
        count.compilations += 1;
        count.compile_seconds += started.elapsed().as_secs_f64();
    }
    #[cfg(windows)]
    if let Some(disk) = disk {
        disk.write(&value);
    }
    if let Ok(mut cache) = cache.lock() {
        cache.insert(key, value.clone());
    }
    Ok(value)
}

fn compile_stage(text: &str, stage: u8) -> Result<Compiled> {
    #[cfg(windows)]
    if stage == 5 {
        return super::compile_bytecode(text, c"cs_5_0");
    }
    super::compile_uncached(text, stage == 1)
}

#[cfg(windows)]
pub(super) fn compute(text: &str) -> Result<Compiled> {
    cached(text, 5)
}
