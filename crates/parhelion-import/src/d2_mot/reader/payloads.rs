//! Retained payloads are bounded independently of package indexes and caller-held reads.
use super::*;
use std::collections::BTreeSet;

pub(super) struct Payloads {
    entries: BTreeMap<u32, (Arc<Payload>, u64)>,
    order: BTreeSet<(u64, u32)>,
    clock: u64,
    bytes: usize,
    limit: usize,
    count: usize,
}

impl Default for Payloads {
    fn default() -> Self {
        Self::new(64 << 20, 4096)
    }
}

impl Payloads {
    pub(super) fn new(limit: usize, count: usize) -> Self {
        Self {
            entries: BTreeMap::new(),
            order: BTreeSet::new(),
            clock: 0,
            bytes: 0,
            limit,
            count,
        }
    }

    pub(super) fn get(&mut self, tag: u32) -> Option<Arc<Payload>> {
        let (payload, used) = self.entries.get_mut(&tag)?;
        self.order.remove(&(*used, tag));
        self.clock += 1;
        *used = self.clock;
        self.order.insert((*used, tag));
        Some(Arc::clone(payload))
    }

    pub(super) fn insert(&mut self, tag: u32, payload: Arc<Payload>) {
        if payload.0.len() > self.limit || self.count == 0 {
            return;
        }
        if let Some((old, used)) = self.entries.remove(&tag) {
            self.bytes -= old.0.len();
            self.order.remove(&(used, tag));
        }
        while self.bytes + payload.0.len() > self.limit || self.entries.len() >= self.count {
            let Some((_, oldest)) = self.order.pop_first() else {
                break;
            };
            if let Some((old, _)) = self.entries.remove(&oldest) {
                self.bytes -= old.0.len();
            }
        }
        self.clock += 1;
        self.bytes += payload.0.len();
        self.order.insert((self.clock, tag));
        self.entries.insert(tag, (payload, self.clock));
    }

    pub(super) fn clear(&mut self) {
        self.entries.clear();
        self.order.clear();
        self.bytes = 0;
    }
}

#[cfg(test)]
mod tests;
