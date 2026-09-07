use std::sync::Arc;

use crate::{catalog::Catalog, hash::format_hash_hex};

use super::matches::CatalogHashMatchIndex;
use crate::app::inspector::DefinitionInspectionContext;

#[derive(Clone, Debug)]
pub(super) struct InspectionTarget {
    pub(super) hash: u64,
    context: Option<DefinitionInspectionContext>,
}
#[derive(Debug, Default)]
pub(in crate::app) struct HashInspectionState {
    pub(super) current: Option<u64>,
    pub(super) history: Vec<InspectionTarget>,
    pub(super) forward: Vec<InspectionTarget>,
    pub(super) match_index: Option<(u64, Arc<CatalogHashMatchIndex>)>,
    pub(super) lookup: String,
    pub(super) lookup_error: bool,
    pub(super) source_context: Option<DefinitionInspectionContext>,
    pub(super) mutation_feedback: Option<(bool, String)>,
    pub(super) runtime: super::runtime::RuntimeInspectionState,
}

pub(super) const HASH_INSPECTOR_HISTORY_LIMIT: usize = 32;

impl HashInspectionState {
    pub(in crate::app) fn open(&mut self, hash: u64) {
        self.open_with_context(hash, None);
    }

    pub(in crate::app) fn open_with_context(
        &mut self,
        hash: u64,
        context: Option<DefinitionInspectionContext>,
    ) {
        if hash == 0 {
            return;
        }
        if self.current == Some(hash) && (context.is_none() || context == self.source_context) {
            return;
        }
        if let Some(current) = self.current {
            self.history.push(InspectionTarget {
                hash: current,
                context: self.source_context.take(),
            });
            trim_navigation_stack(&mut self.history);
        }
        self.forward.clear();
        self.current = Some(hash);
        self.match_index = None;
        self.lookup = format_hash_hex(hash);
        self.lookup_error = false;
        self.source_context = context;
        self.mutation_feedback = None;
    }

    pub(in crate::app) const fn is_open(&self) -> bool {
        self.current.is_some()
    }

    pub(super) fn back(&mut self) {
        if let Some(previous) = self.history.pop() {
            if let Some(current) = self.current {
                self.forward.push(InspectionTarget {
                    hash: current,
                    context: self.source_context.take(),
                });
                trim_navigation_stack(&mut self.forward);
            }
            self.current = Some(previous.hash);
            self.match_index = None;
            self.lookup = format_hash_hex(previous.hash);
            self.lookup_error = false;
            self.source_context = previous.context;
            self.mutation_feedback = None;
        }
    }

    pub(super) fn forward(&mut self) {
        if let Some(next) = self.forward.pop() {
            if let Some(current) = self.current {
                self.history.push(InspectionTarget {
                    hash: current,
                    context: self.source_context.take(),
                });
                trim_navigation_stack(&mut self.history);
            }
            self.current = Some(next.hash);
            self.match_index = None;
            self.lookup = format_hash_hex(next.hash);
            self.lookup_error = false;
            self.source_context = next.context;
            self.mutation_feedback = None;
        }
    }

    pub(super) fn navigate_history(&mut self, history_index: usize) {
        if self.history.get(history_index).is_none() {
            return;
        }
        let newer_history = self.history.split_off(history_index + 1);
        let target = self
            .history
            .pop()
            .expect("selected history entry remains after splitting newer entries");
        if let Some(current) = self.current {
            self.forward.push(InspectionTarget {
                hash: current,
                context: self.source_context.take(),
            });
        }
        self.forward.extend(newer_history.into_iter().rev());
        trim_navigation_stack(&mut self.forward);
        self.current = Some(target.hash);
        self.match_index = None;
        self.lookup = format_hash_hex(target.hash);
        self.lookup_error = false;
        self.source_context = target.context;
        self.mutation_feedback = None;
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
        self.runtime.clear();
        self.current = None;
        self.history.clear();
        self.forward.clear();
        self.match_index = None;
        self.lookup.clear();
        self.lookup_error = false;
        self.source_context = None;
        self.mutation_feedback = None;
    }
}

fn trim_navigation_stack(stack: &mut Vec<InspectionTarget>) {
    let overflow = stack.len().saturating_sub(HASH_INSPECTOR_HISTORY_LIMIT);
    if overflow > 0 {
        stack.drain(0..overflow);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::inspector::DefinitionInspectionContext;

    fn assert_navigation_state(
        inspection: &HashInspectionState,
        current: Option<u64>,
        history: &[u64],
        forward: &[u64],
    ) {
        assert_eq!(inspection.current, current);
        assert_eq!(hashes(&inspection.history), history);
        assert_eq!(hashes(&inspection.forward), forward);
    }

    #[test]
    fn navigation_keeps_a_real_history() {
        let mut inspection = HashInspectionState::default();
        inspection.open(0);
        assert_eq!(inspection.current, None);

        inspection.open(0x1111_1111);
        inspection.open(0x1111_1111);
        assert_navigation_state(&inspection, Some(0x1111_1111), &[], &[]);

        inspection.open(0x2222_2222);
        inspection.open(0x3333_3333);
        assert_navigation_state(
            &inspection,
            Some(0x3333_3333),
            &[0x1111_1111, 0x2222_2222],
            &[],
        );

        inspection.back();
        assert_navigation_state(
            &inspection,
            Some(0x2222_2222),
            &[0x1111_1111],
            &[0x3333_3333],
        );

        inspection.open(0x3333_3333);
        inspection.open(0x4444_4444);
        inspection.navigate_history(0);
        assert_navigation_state(
            &inspection,
            Some(0x1111_1111),
            &[],
            &[0x4444_4444, 0x3333_3333, 0x2222_2222],
        );
        assert_eq!(inspection.lookup, "0x11111111");

        inspection.close();
        assert!(!inspection.is_open());
        assert!(inspection.history.is_empty());
        assert!(inspection.forward.is_empty());
        assert!(inspection.lookup.is_empty());
        assert!(!inspection.lookup_error);
    }

    #[test]
    fn navigation_restores_the_original_instance_snapshot() {
        let mut inspection = HashInspectionState::default();
        let context = DefinitionInspectionContext {
            source: "Character 1 equipment · Kinetic".into(),
            instance_id: Some("0x4000000000000001".into()),
            authored_level: Some(1_950),
            flags: Some(1),
            plug_count: Some(8),
            ..Default::default()
        };

        inspection.open_with_context(0xD980_2C4F, Some(context.clone()));
        assert_eq!(inspection.source_context, Some(context.clone()));

        inspection.open(0x395D_3E2F);
        assert_eq!(inspection.source_context, None);

        inspection.back();
        assert_eq!(inspection.current, Some(0xD980_2C4F));
        assert_eq!(inspection.source_context, Some(context));
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
        assert_eq!(hashes(&inspection.forward), [0x3333_3333, 0x2222_2222]);

        inspection.forward();
        assert_eq!(inspection.current, Some(0x2222_2222));
        assert_eq!(hashes(&inspection.history), [0x1111_1111]);
        assert_eq!(hashes(&inspection.forward), [0x3333_3333]);

        inspection.forward();
        assert_eq!(inspection.current, Some(0x3333_3333));
        assert_eq!(hashes(&inspection.history), [0x1111_1111, 0x2222_2222]);
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
        assert_eq!(hashes(&inspection.history).first(), Some(&8));
        assert_eq!(hashes(&inspection.history).last(), Some(&39));
    }

    fn hashes(targets: &[InspectionTarget]) -> Vec<u64> {
        targets.iter().map(|entry| entry.hash).collect()
    }

    #[test]
    fn same_definition_instances_keep_separate_history_snapshots() {
        let mut state = HashInspectionState::default();
        let first = DefinitionInspectionContext {
            instance_id: Some("first".into()),
            plugs: Some(serde_json::json!([1, null])),
            ..Default::default()
        };
        let second = DefinitionInspectionContext {
            instance_id: Some("second".into()),
            plugs: Some(serde_json::json!([2])),
            ..Default::default()
        };
        state.open_with_context(7, Some(first.clone()));
        state.open_with_context(7, Some(second.clone()));
        state.open(0);
        assert_eq!(state.source_context, Some(second.clone()));
        state.back();
        assert_eq!(state.source_context, Some(first));
        state.forward();
        assert_eq!(state.source_context, Some(second));
    }
}
