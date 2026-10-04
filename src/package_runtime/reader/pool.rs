//! Weighted reader LRU. Active readers remain counted and cannot be evicted.
use std::{
    collections::VecDeque,
    sync::{Arc, Condvar, Mutex},
};
type Key = (u64, u16);
struct Entry<V> {
    key: Key,
    cost: usize,
    /// An in-flight open reserves its handle budget without holding the pool lock.
    value: Option<Arc<V>>,
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
                if entries[index].value.is_none() {
                    entries = self.ready.wait(entries).unwrap_or_else(|e| e.into_inner());
                    continue;
                }
                let entry = entries.remove(index).expect("located reader");
                let value = Arc::clone(entry.value.as_ref().expect("opened reader"));
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
            if let Some(index) = entries.iter().position(|e| {
                e.value
                    .as_ref()
                    .is_some_and(|value| Arc::strong_count(value) == 1)
            }) {
                entries.remove(index);
            } else {
                entries = self.ready.wait(entries).unwrap_or_else(|e| e.into_inner());
            }
        }
        entries.push_back(Entry {
            key,
            cost,
            value: None,
        });
        drop(entries);
        // Opening a package can read its header and patch chain. Unrelated ready readers
        // remain available throughout that I/O, including to the authoring UI.
        let reservation = Reservation { pool: self, key };
        let value = Arc::new(open()?);
        {
            let mut entries = self.entries.lock().unwrap_or_else(|e| e.into_inner());
            let entry = entries
                .iter_mut()
                .find(|entry| entry.key == key)
                .expect("reserved reader");
            entry.value = Some(Arc::clone(&value));
        }
        drop(reservation);
        self.ready.notify_all();
        Ok(Lease {
            value: Some(value),
            pool: self,
        })
    }
    pub fn remove_owner(&self, owner: u64) {
        let mut entries = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        entries.retain(|e| e.key.0 != owner || e.value.is_none());
        self.ready.notify_all();
    }
}

/// Releases a failed or panicking open's reservation. A committed value stays cached.
struct Reservation<'a, V> {
    pool: &'a Pool<V>,
    key: Key,
}
impl<V> Drop for Reservation<'_, V> {
    fn drop(&mut self) {
        let mut entries = self.pool.entries.lock().unwrap_or_else(|e| e.into_inner());
        entries.retain(|entry| entry.key != self.key || entry.value.is_some());
        self.pool.ready.notify_all();
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
