use super::{state::*, table_ui::*, *};
use std::collections::BTreeMap;
mod edits;
mod model;
#[cfg(test)]
mod tests;
mod view;
use model::*;
pub(super) use view::draw;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Mode {
    All,
    Overrides(InvestmentTable),
}

#[derive(Debug, Default)]
pub(super) struct State {
    rows: Option<Vec<Row>>,
    preparing: Option<Preparing>,
    mode: Option<Mode>,
    filtered: Option<(Filter, Vec<usize>)>,
    kind: Option<Kind>,
    scope: Option<&'static str>,
    selected: Option<Key>,
    pub(super) edit_extra: bool,
    feedback: Option<String>,
}
impl State {
    pub fn invalidate(&mut self) {
        self.rows = None;
        self.preparing = None;
        self.filtered = None;
    }
    pub fn reset(&mut self) {
        self.invalidate();
        self.selected = None;
        self.feedback = None;
        self.edit_extra = false;
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Filter {
    query: String,
    kind: Option<Kind>,
    scope: Option<&'static str>,
    mode: Mode,
    sort: TableSort,
    coverage: OverrideFilter,
}
