//! Weighted reader LRU. Active readers remain counted and cannot be evicted.
use std::{
    collections::VecDeque,
    sync::{Arc, Condvar, Mutex},
};
type Key = (u64, u16);
struct Entry<V> {
    key: Key,
    cost: usize,
    value: Arc<V>,
}
pub(super) struct Pool<V> {
    budget: usize,
    entries: Mutex<VecDeque<Entry<V>>>,
    ready: Condvar,
}
impl<V> Pool<V> {
    pub const fn new(budget: usize) -> Self {
        Self {
            budget,
            entries: Mutex::new(VecDeque::new()),
            ready: Condvar::new(),
        }
    }
    pub fn acquire(
        &self,
        key: Key,
        cost: usize,
        open: impl FnOnce() -> Result<V, String>,
    ) -> Result<Lease<'_, V>, String> {
        let mut entries = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            if let Some(index) = entries.iter().position(|e| e.key == key) {
                let entry = entries.remove(index).expect("located reader");
                let value = Arc::clone(&entry.value);
                entries.push_back(entry);
                return Ok(Lease {
                    value: Some(value),
                    pool: self,
                });
            }
            let used: usize = entries.iter().map(|e| e.cost).sum();
            // A single unusually large patch family runs alone rather than deadlocking.
            if used + cost <= self.budget.max(cost) {
                break;
            }
            if let Some(index) = entries
                .iter()
                .position(|e| Arc::strong_count(&e.value) == 1)
            {
                entries.remove(index);
            } else {
                entries = self.ready.wait(entries).unwrap_or_else(|e| e.into_inner());
            }
        }
        let value = Arc::new(open()?);
        entries.push_back(Entry {
            key,
            cost,
            value: Arc::clone(&value),
        });
        Ok(Lease {
            value: Some(value),
            pool: self,
        })
    }
    pub fn remove_owner(&self, owner: u64) {
        let mut entries = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        entries.retain(|e| e.key.0 != owner);
        self.ready.notify_all();
    }
}
pub(super) struct Lease<'a, V> {
    value: Option<Arc<V>>,
    pool: &'a Pool<V>,
}
impl<V> Lease<'_, V> {
    pub fn value(&self) -> &V {
        self.value.as_deref().expect("live reader lease")
    }
}
impl<V> Drop for Lease<'_, V> {
    fn drop(&mut self) {
        // Synchronize release with the waiter's budget check to avoid lost wakeups.
        let _entries = self.pool.entries.lock().unwrap_or_else(|e| e.into_inner());
        self.value.take();
        self.pool.ready.notify_all();
    }
}
#[cfg(test)]
mod tests;
