use super::*;
pub(super) use crate::persistence::progression::InvestmentTable;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::app) enum View {
    Unlocks,
    Investment,
    Triumphs,
}

#[derive(Debug, Default)]
pub(in crate::app) struct UiState {
    pub(in crate::app) read_only: bool,
    pub(super) storage: super::storage::State,
    pub(super) triumphs: super::triumphs::State,
    pub(super) unlock_browser: super::unlocks::Browser,
    pub(super) seasonal: seasonal::UiState,
    pub(super) investment_table: InvestmentTable,
    pub(super) query: String,
    pub(super) table_sorts: HashMap<&'static str, TableSort>,
    pub(super) add_open: bool,
    pub(super) add_query: String,
    pub(super) add_value: i32,
    pub(super) cached_progression: Option<Result<Progression, String>>,
    pub(super) cached_view: Option<(usize, Value)>,
    pub(super) metadata_inspector: ProgressionInspectorState,
    pub(super) hash_inspection: HashInspectionState,
    pub(super) override_filter: OverrideFilter,
    pub(super) last_investment_change: Option<InvestmentUndo>,
    pub(super) progression_baselines: HashMap<(&'static str, usize), Option<[i32; 3]>>,
    pub(super) last_progression_change: Option<ProgressionUndo>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum InvestmentUndo {
    Flag {
        definition_index: usize,
        previous: Option<u8>,
    },
    Value {
        definition_index: usize,
        previous: Option<i32>,
    },
}

impl InvestmentUndo {
    pub(super) fn label(self) -> String {
        match self {
            Self::Flag {
                definition_index,
                previous,
            } => previous.map_or_else(
                || format!("Remove newly added flag #{definition_index}"),
                |value| format!("Restore flag #{definition_index} to {value}"),
            ),
            Self::Value {
                definition_index,
                previous,
            } => previous.map_or_else(
                || format!("Remove newly added value #{definition_index}"),
                |value| format!("Restore value #{definition_index} to {value}"),
            ),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ProgressionUndo {
    pub(super) table: &'static str,
    pub(super) definition_index: usize,
    pub(super) previous: Option<[i32; 3]>,
}

impl UiState {
    pub(in crate::app) fn reset_navigation(&mut self) {
        self.cached_view = None;
        self.storage.reset();
        self.triumphs.reset();
        self.unlock_browser.reset();
        self.query.clear();
        self.add_open = false;
        self.add_query.clear();
        self.metadata_inspector.reset();

        self.hash_inspection.close();
    }

    pub(in crate::app) fn invalidate_document(&mut self) {
        self.seasonal.invalidate();
        self.invalidate_cache();
        self.progression_baselines.clear();
        self.last_progression_change = None;
    }

    pub(in crate::app) fn invalidate_cache(&mut self) {
        self.storage.invalidate();
        self.cached_view = None;
        self.triumphs.invalidate();
        self.cached_progression = None;
        self.unlock_browser.invalidate();
    }

    pub(in crate::app) fn mark_saved(&mut self) {
        self.progression_baselines.clear();
        self.last_progression_change = None;
    }

    pub(super) fn record_progression_change(
        &mut self,
        table: &'static str,
        definition_index: usize,
        previous: Option<[i32; 3]>,
        current: Option<[i32; 3]>,
    ) {
        let key = (table, definition_index);
        let baseline = *self.progression_baselines.entry(key).or_insert(previous);
        if current == baseline {
            self.progression_baselines.remove(&key);
        }

        let keep_previous = self.last_progression_change.is_some_and(|change| {
            change.table == table && change.definition_index == definition_index
        });
        if !keep_previous {
            self.last_progression_change = Some(ProgressionUndo {
                table,
                definition_index,
                previous,
            });
        }
    }

    #[cfg(test)]
    pub(super) fn progression_changed(&self, table: &'static str, definition_index: usize) -> bool {
        self.progression_baselines
            .contains_key(&(table, definition_index))
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct TableSort {
    pub(super) column: usize,
    pub(super) descending: bool,
}

impl TableSort {
    pub(super) const fn ascending(column: usize) -> Self {
        Self {
            column,
            descending: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ProgressionDisplayRow {
    pub(super) definition_index: usize,
    pub(super) lanes: Option<[i32; 3]>,
}
