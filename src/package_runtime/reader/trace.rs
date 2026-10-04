//! Optional content receipts for callers that cache work derived from native tags.
use super::PackageManager;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

#[derive(Default)]
pub(super) struct Reads {
    hashes: BTreeMap<u32, [u8; 32]>,
    failed: bool,
}

impl Reads {
    pub(super) fn record(&mut self, tag: u32, result: &Result<Vec<u8>, String>) {
        match result {
            Ok(bytes) => {
                let hash = Sha256::digest(bytes).into();
                if self.hashes.insert(tag, hash).is_some_and(|old| old != hash) {
                    self.failed = true;
                }
            }
            // Even an optional failed read can affect a fallback. Do not cache that operation.
            Err(_) => self.failed = true,
        }
    }
}

pub struct ReadTrace<'a> {
    manager: &'a PackageManager,
}

impl PackageManager {
    /// Record successful payload reads until the guard finishes or is dropped. Nested
    /// recordings are refused. Metadata lookups must be fingerprinted by the caller too.
    pub fn trace_reads(&self) -> Option<ReadTrace<'_>> {
        let mut trace = self.trace.lock().ok()?;
        if trace.is_some() {
            return None;
        }
        *trace = Some(Reads::default());
        Some(ReadTrace { manager: self })
    }
}

impl ReadTrace<'_> {
    pub fn finish(self) -> Option<BTreeMap<u32, [u8; 32]>> {
        let reads = self.manager.trace.lock().ok()?.take()?;
        (!reads.failed).then_some(reads.hashes)
    }
}

impl Drop for ReadTrace<'_> {
    fn drop(&mut self) {
        if let Ok(mut trace) = self.manager.trace.lock() {
            *trace = None;
        }
    }
}
