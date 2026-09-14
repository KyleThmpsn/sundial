use super::{hierarchy::normalize_hierarchy_path, table_ui::*, *};
use crate::catalog::RecordDefinition;
use std::collections::{BTreeMap, HashSet};

mod edit;
mod model;
#[cfg(test)]
mod tests;
mod view;
use model::*;
pub(super) use view::draw;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Status {
    Completed,
    ObjectivesComplete,
    NotCompleted,
    Unresolved,
}

impl Status {
    const ALL: [Self; 4] = [
        Self::Completed,
        Self::ObjectivesComplete,
        Self::NotCompleted,
        Self::Unresolved,
    ];
    fn label(self) -> &'static str {
        match self {
            Self::Completed => "Completed",
            Self::ObjectivesComplete => "Objectives Complete",
            Self::NotCompleted => "Not Completed",
            Self::Unresolved => "Unresolved",
        }
    }
}

#[derive(Debug)]
struct Row {
    record: RecordDefinition,
    name: String,
    status: Status,
    completed_objectives: usize,
    values: Vec<Option<i32>>,
    search: String,
}

#[derive(Debug, Default)]
pub(super) struct State {
    query: String,
    filter: Option<Status>,
    rows: Option<Vec<Row>>,
    filtered: Option<(Filter, Vec<usize>)>,
    tree: Option<Branch>,
    visible: Option<Vec<Line>>,
    expansion: HashSet<Vec<String>>,
    expanded_records: HashSet<u64>,
    navigation_initialized: bool,
    sort: super::state::TableSort,
    selected: HashSet<u64>,
    job: Option<edit::Job>,
    ready: Option<edit::Job>,
    feedback: Option<(bool, String)>,
    completed: usize,
}

impl State {
    pub fn invalidate(&mut self) {
        self.rows = None;
        self.filtered = None;
        self.tree = None;
        self.visible = None;
    }

    pub fn reset(&mut self) {
        self.invalidate();
        self.selected.clear();
        self.job = None;
        self.ready = None;
        self.feedback = None;
    }
}

fn record_name(record: &RecordDefinition) -> String {
    if record.name.trim().is_empty() {
        format!("Unnamed Triumph #{}", record.index)
    } else {
        record.name.clone()
    }
}

fn display_name(record: &RecordDefinition, catalog: &Catalog) -> String {
    if !record.name.trim().is_empty() {
        return record.name.clone();
    }
    if let Some(name) = catalog
        .display_name(record.hash)
        .filter(|name| !name.trim().is_empty())
    {
        return name.to_owned();
    }
    if let Some(text) = record
        .objectives
        .iter()
        .filter_map(|index| catalog.objective_definition(*index))
        .flat_map(|objective| {
            [
                &objective.name,
                &objective.progress_description,
                &objective.description,
            ]
        })
        .find(|text| !text.trim().is_empty())
    {
        return format!("{text} · #{}", record.index);
    }
    record_name(record)
}

fn objective_complete(objective: &ObjectiveDef, value: i32) -> bool {
    if objective.is_counting_downward {
        value <= objective.completion_value
    } else {
        value >= objective.completion_value
    }
}

pub(super) fn related_changes(
    before: &CollectionStateSnapshot,
    after: &CollectionStateSnapshot,
    catalog: &Catalog,
    selected: &HashSet<u64>,
) -> Vec<super::impact::Change> {
    catalog
        .records()
        .unwrap_or_default()
        .iter()
        .filter(|record| !selected.contains(&record.hash))
        .filter_map(|record| {
            let old = model::progress(record, catalog, before);
            let new = model::progress(record, catalog, after);
            if old == new {
                return None;
            }
            let text = |progress: &model::Progress| {
                let values = progress
                    .values
                    .iter()
                    .zip(&record.objectives)
                    .map(
                        |(value, index)| match (value, catalog.objective_definition(*index)) {
                            (Some(value), Some(objective)) => {
                                format!("{value}/{}", objective.completion_value)
                            }
                            _ => "Unknown".into(),
                        },
                    )
                    .collect::<Vec<_>>()
                    .join(", ");
                if values.is_empty() {
                    progress.status.label().into()
                } else {
                    format!("{} · {values}", progress.status.label())
                }
            };
            Some(super::impact::Change {
                name: display_name(record, catalog),
                before: text(&old),
                after: text(&new),
            })
        })
        .collect()
}

#[cfg(test)]
pub(super) fn edit_benchmark(document: &Value, catalog: &Catalog) {
    let mut job = edit::Job::new(document, catalog.records().unwrap().to_vec(), true);
    let start = std::time::Instant::now();
    let mut worst = std::time::Duration::ZERO;
    let mut frames = 0;
    loop {
        let slice = std::time::Instant::now();
        let done = job.step(catalog);
        worst = worst.max(slice.elapsed());
        frames += 1;
        if done {
            break;
        }
    }
    eprintln!(
        "Triumphs bulk: {} supported, {} unsupported, {} related entries, {} slices, {:?} total, {:?} slowest slice",
        job.supported_count(),
        job.issues.len(),
        job.review.as_ref().unwrap().related.len(),
        frames,
        start.elapsed(),
        worst
    );
    let mut reasons = BTreeMap::new();
    for issue in &job.issues {
        *reasons.entry(issue.reason.clone()).or_insert(0) += 1;
    }
    eprintln!("Triumph edit failures: {reasons:?}");
}
