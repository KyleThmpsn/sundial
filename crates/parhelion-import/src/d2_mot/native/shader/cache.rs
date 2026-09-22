//! Bounded process-local cache. A fresh process always uses the installed compiler.
use anyhow::Result;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    sync::{Mutex, OnceLock},
};

type Compiled = (Vec<u8>, String);
const MAX_BYTES: usize = 32 * 1024 * 1024;
#[derive(Default)]
struct Cache {
    entries: BTreeMap<[u8; 32], Compiled>,
    bytes: usize,
}
impl Cache {
    fn insert(&mut self, key: [u8; 32], value: Compiled) {
        let size = value.0.len() + value.1.len();
        if size > MAX_BYTES || self.entries.contains_key(&key) {
            return;
        }
        if self.bytes + size > MAX_BYTES {
            self.entries.clear();
            self.bytes = 0;
        }
        self.bytes += size;
        self.entries.insert(key, value);
    }
}
fn key(text: &str, vertex: bool) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update([u8::from(vertex)]);
    hash.update(text.as_bytes());
    hash.finalize().into()
}
pub(super) fn compile(text: &str, vertex: bool) -> Result<Compiled> {
    static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
    let cache = CACHE.get_or_init(Mutex::default);
    let key = key(text, vertex);
    if let Ok(cache) = cache.lock() {
        if let Some(value) = cache.entries.get(&key) {
            return Ok(value.clone());
        }
    }
    // Do not hold a global lock during compilation. Errors are never cached.
    let value = super::compile_uncached(text, vertex)?;
    if let Ok(mut cache) = cache.lock() {
        cache.insert(key, value.clone());
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cache_separates_programs_and_stages_and_bounds_memory() {
        let mut cache = Cache::default();
        let first = key("source", true);
        cache.insert(first, (vec![1; MAX_BYTES - 1], String::new()));
        cache.insert(first, (vec![2], String::new()));
        assert_eq!(cache.bytes, MAX_BYTES - 1);
        assert_ne!(first, key("source", false));
        assert_ne!(first, key("other source", true));
        let second = key("other source", true);
        cache.insert(second, (vec![3; 2], String::new()));
        assert_eq!(cache.bytes, 2);
        assert!(!cache.entries.contains_key(&first));
        assert_eq!(cache.entries[&second].0, [3; 2]);
    }
}
