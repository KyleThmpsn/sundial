use std::sync::Arc;

use crate::{catalog::Catalog, hash::format_hash_hex};

use super::matches::CatalogHashMatchIndex;
#[derive(Debug, Default)]
pub(in crate::app) struct HashInspectionState {
    pub(super) current: Option<u64>,
    pub(super) history: Vec<u64>,
    pub(super) forward: Vec<u64>,
    pub(super) match_index: Option<(u64, Arc<CatalogHashMatchIndex>)>,
    pub(super) lookup: String,
    pub(super) lookup_error: bool,
}

pub(super) const HASH_INSPECTOR_HISTORY_LIMIT: usize = 32;

impl HashInspectionState {
    pub(in crate::app) fn open(&mut self, hash: u64) {
        if hash == 0 || self.current == Some(hash) {
            return;
        }
        if let Some(current) = self.current {
            self.history.push(current);
            trim_navigation_stack(&mut self.history);
        }
        self.forward.clear();
        self.current = Some(hash);
        self.match_index = None;
        self.lookup = format_hash_hex(hash);
        self.lookup_error = false;
    }

    pub(in crate::app) const fn is_open(&self) -> bool {
        self.current.is_some()
    }

    pub(super) fn back(&mut self) {
        if let Some(previous) = self.history.pop() {
            if let Some(current) = self.current {
                self.forward.push(current);
                trim_navigation_stack(&mut self.forward);
            }
            self.current = Some(previous);
            self.match_index = None;
            self.lookup = format_hash_hex(previous);
            self.lookup_error = false;
        }
    }

    pub(super) fn forward(&mut self) {
        if let Some(next) = self.forward.pop() {
            if let Some(current) = self.current {
                self.history.push(current);
                trim_navigation_stack(&mut self.history);
            }
            self.current = Some(next);
            self.match_index = None;
            self.lookup = format_hash_hex(next);
            self.lookup_error = false;
        }
    }

    pub(super) fn navigate_history(&mut self, history_index: usize) {
        if self.history.get(history_index).is_none() {
            return;
        }
        let newer_history = self.history.split_off(history_index + 1);
        let hash = self
            .history
            .pop()
            .expect("selected history entry remains after splitting newer entries");
        if let Some(current) = self.current {
            self.forward.push(current);
        }
        self.forward.extend(newer_history.into_iter().rev());
        trim_navigation_stack(&mut self.forward);
        self.current = Some(hash);
        self.match_index = None;
        self.lookup = format_hash_hex(hash);
        self.lookup_error = false;
    }

    pub(super) fn match_index(
        &mut self,
        catalog: &Catalog,
        hash: u64,
    ) -> Arc<CatalogHashMatchIndex> {
        if self
            .match_index
            .as_ref()
            .is_none_or(|(cached_hash, _)| *cached_hash != hash)
        {
            self.match_index = Some((
                hash,
                Arc::new(CatalogHashMatchIndex::collect(catalog, hash)),
            ));
        }
        Arc::clone(&self.match_index.as_ref().expect("match index was set").1)
    }

    pub(in crate::app) fn close(&mut self) {
        self.current = None;
        self.history.clear();
        self.forward.clear();
        self.match_index = None;
        self.lookup.clear();
        self.lookup_error = false;
    }
}

fn trim_navigation_stack(stack: &mut Vec<u64>) {
    let overflow = stack.len().saturating_sub(HASH_INSPECTOR_HISTORY_LIMIT);
    if overflow > 0 {
        stack.drain(0..overflow);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn navigation_keeps_a_real_history() {
        let mut inspection = HashInspectionState::default();
        inspection.open(0);
        assert_eq!(inspection.current, None);

        inspection.open(0x1111_1111);
        inspection.open(0x1111_1111);
        assert_eq!(inspection.current, Some(0x1111_1111));
        assert!(inspection.history.is_empty());

        inspection.open(0x2222_2222);
        inspection.open(0x3333_3333);
        assert_eq!(inspection.current, Some(0x3333_3333));
        assert_eq!(inspection.history, [0x1111_1111, 0x2222_2222]);
        assert!(inspection.forward.is_empty());

        inspection.back();
        assert_eq!(inspection.current, Some(0x2222_2222));
        assert_eq!(inspection.history, [0x1111_1111]);
        assert_eq!(inspection.forward, [0x3333_3333]);

        inspection.open(0x3333_3333);
        inspection.open(0x4444_4444);
        inspection.navigate_history(0);
        assert_eq!(inspection.current, Some(0x1111_1111));
        assert!(inspection.history.is_empty());
        assert_eq!(inspection.forward, [0x4444_4444, 0x3333_3333, 0x2222_2222]);
        assert_eq!(inspection.lookup, "0x11111111");

        inspection.close();
        assert!(!inspection.is_open());
        assert!(inspection.history.is_empty());
        assert!(inspection.forward.is_empty());
        assert!(inspection.lookup.is_empty());
        assert!(!inspection.lookup_error);
    }

    #[test]
    fn navigation_supports_back_and_forward() {
        let mut inspection = HashInspectionState::default();
        inspection.open(0x1111_1111);
        inspection.open(0x2222_2222);
        inspection.open(0x3333_3333);

        inspection.back();
        inspection.back();
        assert_eq!(inspection.current, Some(0x1111_1111));
        assert_eq!(inspection.forward, [0x3333_3333, 0x2222_2222]);

        inspection.forward();
        assert_eq!(inspection.current, Some(0x2222_2222));
        assert_eq!(inspection.history, [0x1111_1111]);
        assert_eq!(inspection.forward, [0x3333_3333]);

        inspection.forward();
        assert_eq!(inspection.current, Some(0x3333_3333));
        assert_eq!(inspection.history, [0x1111_1111, 0x2222_2222]);
        assert!(inspection.forward.is_empty());

        inspection.open(0x4444_4444);
        assert!(inspection.forward.is_empty());
    }

    #[test]
    fn navigation_history_is_bounded_to_recent_definitions() {
        let mut inspection = HashInspectionState::default();
        for hash in 1..=40 {
            inspection.open(hash);
        }
        assert_eq!(inspection.current, Some(40));
        assert_eq!(inspection.history.len(), HASH_INSPECTOR_HISTORY_LIMIT);
        assert_eq!(inspection.history.first(), Some(&8));
        assert_eq!(inspection.history.last(), Some(&39));
    }
}
