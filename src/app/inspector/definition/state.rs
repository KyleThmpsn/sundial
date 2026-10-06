use std::{
    collections::{BTreeMap, HashMap, HashSet},
    sync::Arc,
};

use eframe::egui;

use crate::catalog::{BrowseEntry, Catalog, DefinitionSearchHit, ItemRarity, Shelf};

use super::matches::CatalogHashMatchIndex;
use crate::app::inspector::DefinitionInspectionContext;

/// The search page, kept in navigation like a definition. No definition has hash zero.
pub(super) const HOME: u64 = 0;

#[derive(Clone, Debug)]
pub(super) struct InspectionTarget {
    pub(super) hash: u64,
    context: Option<DefinitionInspectionContext>,
}
#[derive(Debug, Default)]
pub(in crate::app) struct HashInspectionState {
    /// The inspected hash, or [`HOME`] while the search page shows.
    pub(super) current: Option<u64>,
    pub(super) history: Vec<InspectionTarget>,
    pub(super) forward: Vec<InspectionTarget>,
    pub(super) match_index: Option<(u64, Arc<CatalogHashMatchIndex>)>,
    pub(super) search: DefinitionSearch,
    /// The home page's shelves and filters, kept while the window is closed.
    pub(super) browse: Browse,
    pub(super) source_context: Option<DefinitionInspectionContext>,
    pub(super) mutation_feedback: Option<(bool, String)>,
    pub(super) runtime: super::runtime::RuntimeInspectionState,
    /// Window size chosen when the inspector opened, so navigation keeps the user's resize.
    pub(super) default_size: Option<egui::Vec2>,
}

/// The toolbar and home search fields with the results of the last query.
#[derive(Debug, Default)]
pub(super) struct DefinitionSearch {
    /// The toolbar field, cleared once it opens a definition.
    pub(super) query: String,
    /// The home page field, kept so Back returns to the same results.
    pub(super) home_query: String,
    /// Set to focus whichever search field draws next.
    pub(super) focus: bool,
    pub(super) results: SearchResults,
    /// Scroll offset and height of the home result list last frame.
    pub(super) list_view: (f32, f32),
    /// Kind labels of recently opened hashes, each with the item whose icon stands for it.
    pub(super) kinds: HashMap<u64, (&'static str, u64)>,
}

/// Search hits for one query, recomputed only when the query or catalog changes.
#[derive(Debug, Default)]
pub(super) struct SearchResults {
    /// Catalog address and trimmed query the hits belong to.
    pub(super) key: Option<(usize, String)>,
    pub(super) hits: Vec<DefinitionSearchHit>,
    /// A hash the query parses as, offered as a row of its own.
    pub(super) hash: Option<u64>,
    pub(super) highlighted: usize,
}

/// A home page tab: a shelf of items or the recently opened definitions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum BrowseTab {
    Shelf(Shelf),
    Recent,
}

impl Default for BrowseTab {
    fn default() -> Self {
        Self::Shelf(Shelf::default())
    }
}

/// The order cards are listed in. Ordering never hides a card.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(super) enum BrowseSort {
    #[default]
    Type,
    Name,
    Rarity,
}

impl BrowseSort {
    pub(super) const ALL: [Self; 3] = [Self::Type, Self::Name, Self::Rarity];

    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::Type => "By Type",
            Self::Name => "Name",
            Self::Rarity => "Rarity",
        }
    }
}

/// The home page's tab, filters and the cards they leave.
#[derive(Debug, Default)]
pub(super) struct Browse {
    pub(super) tab: BrowseTab,
    /// None shows every type.
    pub(super) type_name: Option<String>,
    pub(super) rarity: Option<ItemRarity>,
    /// 0 Titan, 1 Hunter, 2 Warlock. None shows every class.
    pub(super) class: Option<u8>,
    pub(super) sort: BrowseSort,
    pub(super) dummy_items: bool,
    /// Type groups folded on each shelf.
    pub(super) folded: HashSet<(Shelf, String)>,
    pub(super) index: Option<Arc<BrowseIndex>>,
    pub(super) results: BrowseResults,
    /// Definitions other than items that match the search.
    pub(super) definitions: SearchResults,
}

impl Browse {
    /// Whether a filter or the search narrows the cards beyond the defaults.
    pub(super) fn narrowed(&self, query: &str) -> bool {
        self.type_name.is_some()
            || self.rarity.is_some()
            || self.class.is_some()
            || self.dummy_items
            || !query.trim().is_empty()
    }

    pub(super) fn reset(&mut self) {
        self.type_name = None;
        self.rarity = None;
        self.class = None;
        self.dummy_items = false;
    }
}

/// Every browsable entry of one catalog, with the text the search matches.
#[derive(Debug)]
pub(super) struct BrowseIndex {
    /// The catalog's address, so a new catalog rebuilds the index.
    pub(super) catalog: usize,
    pub(super) entries: Vec<BrowseEntry>,
    /// Lowercased name, type and hash per entry.
    pub(super) text: Vec<String>,
}

/// The cards for one set of filters, rebuilt only when that set changes.
#[derive(Debug, Default)]
pub(super) struct BrowseResults {
    pub(super) key: Option<BrowseKey>,
    /// Indices into the index's entries, in display order.
    pub(super) cards: Vec<usize>,
    /// Cards each type would show, counted before the type filter.
    pub(super) types: BTreeMap<String, usize>,
    /// Cards across every type.
    pub(super) total: usize,
    /// Search matches on each shelf, in [`Shelf::ALL`] order.
    pub(super) shelf_matches: [usize; 5],
    /// Whether the shelf holds items of one class.
    pub(super) has_classes: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct BrowseKey {
    pub(super) catalog: usize,
    pub(super) query: String,
    pub(super) shelf: Shelf,
    pub(super) type_name: Option<String>,
    pub(super) rarity: Option<ItemRarity>,
    pub(super) class: Option<u8>,
    pub(super) sort: BrowseSort,
    pub(super) dummy_items: bool,
}

pub(super) const HASH_INSPECTOR_HISTORY_LIMIT: usize = 32;

impl HashInspectionState {
    /// Opens the window on its search page, or focuses the search field when it is open.
    pub(in crate::app) fn open_search(&mut self) {
        if self.current.is_none() {
            self.select(InspectionTarget {
                hash: HOME,
                context: None,
            });
        }
        self.search.focus = true;
    }

    /// Leaves the inspected definition for the search page, which Back returns from.
    pub(super) fn go_home(&mut self) {
        if self.current != Some(HOME) {
            if let Some(current) = self.take_current() {
                self.history.push(current);
                trim_navigation_stack(&mut self.history);
            }
            self.forward.clear();
            self.select(InspectionTarget {
                hash: HOME,
                context: None,
            });
        }
        self.search.focus = true;
    }

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
        if let Some(current) = self.take_current() {
            self.history.push(current);
            trim_navigation_stack(&mut self.history);
        }
        self.forward.clear();
        self.select(InspectionTarget { hash, context });
    }

    pub(in crate::app) const fn is_open(&self) -> bool {
        self.current.is_some()
    }

    pub(super) fn back(&mut self) {
        if let Some(previous) = self.history.pop() {
            if let Some(current) = self.take_current() {
                self.forward.push(current);
                trim_navigation_stack(&mut self.forward);
            }
            self.select(previous);
        }
    }

    pub(super) fn forward(&mut self) {
        if let Some(next) = self.forward.pop() {
            if let Some(current) = self.take_current() {
                self.history.push(current);
                trim_navigation_stack(&mut self.history);
            }
            self.select(next);
        }
    }

    /// Moves forward to one of the forward entries, where the last entry is the nearest.
    pub(super) fn navigate_forward(&mut self, forward_index: usize) {
        let steps = self.forward.len().saturating_sub(forward_index);
        for _ in 0..steps {
            self.forward();
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
        if let Some(current) = self.take_current() {
            self.forward.push(current);
        }
        self.forward.extend(newer_history.into_iter().rev());
        trim_navigation_stack(&mut self.forward);
        self.select(target);
    }

    fn take_current(&mut self) -> Option<InspectionTarget> {
        self.current.take().map(|hash| InspectionTarget {
            hash,
            context: self.source_context.take(),
        })
    }

    fn select(&mut self, target: InspectionTarget) {
        self.current = Some(target.hash);
        self.match_index = None;
        self.search.query.clear();
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

    /// Closes the window. The back history stays, so the search page lists it when reopened.
    pub(in crate::app) fn close(&mut self) {
        self.runtime.clear();
        if let Some(current) = self.take_current().filter(|target| target.hash != HOME) {
            self.history.push(current);
            trim_navigation_stack(&mut self.history);
        }
        self.current = None;
        self.forward.clear();
        self.match_index = None;
        self.search = DefinitionSearch::default();
        self.source_context = None;
        self.mutation_feedback = None;
        self.default_size = None;
    }

    /// Closes the inspector and forgets its history, for a new account, catalog or page.
    pub(in crate::app) fn reset(&mut self) {
        self.close();
        self.history.clear();
        self.browse = Browse::default();
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

        inspection.navigate_forward(1);
        assert_navigation_state(
            &inspection,
            Some(0x3333_3333),
            &[0x1111_1111, 0x2222_2222],
            &[0x4444_4444],
        );

        inspection.search.query = "ace".into();
        inspection.close();
        assert!(!inspection.is_open());
        assert_eq!(
            hashes(&inspection.history),
            [0x1111_1111, 0x2222_2222, 0x3333_3333],
            "closing keeps the back history and the page that was open"
        );
        assert!(inspection.forward.is_empty());
        assert!(inspection.search.query.is_empty());

        inspection.open_search();
        assert_navigation_state(
            &inspection,
            Some(HOME),
            &[0x1111_1111, 0x2222_2222, 0x3333_3333],
            &[],
        );
        inspection.close();
        assert_eq!(
            hashes(&inspection.history),
            [0x1111_1111, 0x2222_2222, 0x3333_3333],
            "the search page is not kept as a recent definition"
        );
    }

    #[test]
    fn the_search_page_is_a_navigation_entry() {
        let mut inspection = HashInspectionState::default();
        inspection.open_search();
        assert!(inspection.is_open());
        assert_navigation_state(&inspection, Some(HOME), &[], &[]);
        assert!(inspection.search.focus);

        inspection.open(0x1111_1111);
        assert_navigation_state(&inspection, Some(0x1111_1111), &[HOME], &[]);
        inspection.search.focus = false;
        inspection.open_search();
        assert_eq!(
            inspection.current,
            Some(0x1111_1111),
            "an open window keeps its page and focuses the search field"
        );
        assert!(inspection.search.focus);

        inspection.go_home();
        assert_navigation_state(&inspection, Some(HOME), &[HOME, 0x1111_1111], &[]);
        inspection.back();
        assert_navigation_state(&inspection, Some(0x1111_1111), &[HOME], &[HOME]);
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
