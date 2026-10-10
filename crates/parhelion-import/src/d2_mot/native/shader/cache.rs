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
fn key(text: &str, vertex: bool) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update([u8::from(vertex)]);
    hash.update(text.as_bytes());
    hash.finalize().into()
}
pub(super) fn compile(text: &str, vertex: bool) -> Result<Compiled> {
    crate::cancellation::check()?;
    static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
    let cache = CACHE.get_or_init(Mutex::default);
    let key = key(text, vertex);
    if let Ok(mut cache) = cache.lock()
        && let Some(value) = cache.get(&key)
    {
        return Ok(value.clone());
    }
    // Do not hold a global lock during compilation. Errors are never cached.
    let text = text.to_owned();
    let value = crate::cancellation::compute(move || super::compile_uncached(&text, vertex))?;
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
        assert!(cache.bytes <= MAX_BYTES);
        assert!(cache.get(&key("source", false)).is_none());
        assert!(cache.get(&key("other source", true)).is_none());
        let second = key("other source", true);
        cache.insert(second, (vec![3; 2], String::new()));
        assert!(cache.bytes <= MAX_BYTES);
        assert!(cache.get(&first).is_none());
        assert_eq!(cache.get(&second).unwrap().0, [3; 2]);
    }

    #[test]
    fn eviction_preserves_reused_programs_when_cold_programs_can_make_room() {
        let mut cache = Cache::default();
        let cold = key("cold", false);
        let hot = key("reused", true);
        cache.insert(cold, (vec![1; MAX_BYTES / 2], String::new()));
        cache.insert(hot, (vec![2; MAX_BYTES / 2], String::new()));
        assert_eq!(cache.get(&hot).unwrap().0[0], 2);
        cache.insert(key("new", false), (vec![3; 16], String::new()));
        assert_eq!(cache.get(&hot).unwrap().0[0], 2);
        assert!(cache.get(&cold).is_none());
    }
}
