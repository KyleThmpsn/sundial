use super::*;

#[derive(Debug, PartialEq, Eq)]
pub(super) struct Filter {
    pub query: String,
    pub status: Option<Status>,
    pub sort: super::super::state::TableSort,
}

pub(super) fn rows(catalog: &Catalog, snapshot: &CollectionStateSnapshot) -> Vec<Row> {
    catalog
        .records()
        .unwrap_or_default()
        .iter()
        .map(|record| {
            let Progress {
                values,
                completed_objectives,
                status,
            } = progress(record, catalog, snapshot);
            let name = display_name(record, catalog);
            let mut search = format!(
                "{name} {} {} {}",
                record.index,
                record.hash,
                format_hash_hex(record.hash)
            );
            for path in &record.paths {
                search.push_str(&format!(" {}", path.join(" ")));
            }
            for index in &record.objectives {
                if let Some(objective) = catalog.objective_definition(*index) {
                    search.push_str(&format!(
                        " {} {} {}",
                        objective.name, objective.description, objective.progress_description
                    ));
                }
            }
            Row {
                record: record.clone(),
                name,
                status,
                completed_objectives,
                values,
                search: search.to_lowercase(),
            }
        })
        .collect()
}

#[derive(Debug, Default)]
pub(super) struct Branch {
    pub children: BTreeMap<String, Branch>,
    pub records: Vec<usize>,
    pub members: HashSet<usize>,
    pub completed: usize,
}

#[derive(Debug)]
pub(super) enum Line {
    Branch {
        path: Vec<String>,
        depth: usize,
    },
    Record {
        row: usize,
        depth: usize,
    },
    Objective {
        row: usize,
        objective: usize,
        depth: usize,
    },
}

impl Branch {
    pub fn at(&self, path: &[String]) -> &Self {
        path.iter()
            .fold(self, |branch, part| &branch.children[part])
    }
}

pub(super) fn tree(rows: &[Row], filtered: &[usize]) -> Branch {
    let mut root = Branch::default();
    for &index in filtered {
        let paths = if rows[index].record.paths.is_empty() {
            vec![vec!["Uncategorized".into()]]
        } else {
            rows[index]
                .record
                .paths
                .iter()
                .map(|path| {
                    normalize_hierarchy_path(path, "Triumphs")
                        .into_iter()
                        .filter(|part| part != "Triumphs")
                        .collect()
                })
                .collect()
        };
        for path in paths {
            let mut branch = &mut root;
            if branch.members.insert(index) && rows[index].status == Status::Completed {
                branch.completed += 1;
            }
            for part in path {
                branch = branch.children.entry(part).or_default();
                if branch.members.insert(index) && rows[index].status == Status::Completed {
                    branch.completed += 1;
                }
            }
            if !branch.records.contains(&index) {
                branch.records.push(index);
            }
        }
    }
    root
}

pub(super) fn lines(root: &Branch, rows: &[Row], state: &State, auto_expand: bool) -> Vec<Line> {
    fn append(
        branch: &Branch,
        path: &mut Vec<String>,
        rows: &[Row],
        state: &State,
        auto_expand: bool,
        output: &mut Vec<Line>,
    ) {
        for (name, child) in &branch.children {
            path.push(name.clone());
            output.push(Line::Branch {
                path: path.clone(),
                depth: path.len() - 1,
            });
            if auto_expand || state.expansion.contains(path) {
                append(child, path, rows, state, auto_expand, output);
            }
            path.pop();
        }
        for &row in &branch.records {
            output.push(Line::Record {
                row,
                depth: path.len(),
            });
            if state.expanded_records.contains(&rows[row].record.hash) {
                for objective in 0..rows[row].record.objectives.len() {
                    output.push(Line::Objective {
                        row,
                        objective,
                        depth: path.len() + 1,
                    });
                }
            }
        }
    }
    let mut output = Vec::new();
    append(root, &mut Vec::new(), rows, state, auto_expand, &mut output);
    output
}

pub(super) fn filter_rows(rows: &[Row], filter: &Filter) -> Vec<usize> {
    let mut filtered = rows
        .iter()
        .enumerate()
        .filter(|(_, row)| {
            filter.status.is_none_or(|status| status == row.status)
                && filter
                    .query
                    .split_whitespace()
                    .all(|term| row.search.contains(term))
        })
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    match filter.sort.column {
        1 => filtered.sort_by_key(|index| {
            (
                rows[*index].completed_objectives,
                rows[*index].record.objectives.len(),
            )
        }),
        2 => filtered.sort_by_key(|index| rows[*index].status),
        _ => filtered.sort_by_cached_key(|index| rows[*index].name.to_lowercase()),
    }
    if filter.sort.descending {
        filtered.reverse();
    }
    filtered
}

#[derive(PartialEq)]
pub(super) struct Progress {
    pub values: Vec<Option<i32>>,
    pub completed_objectives: usize,
    pub status: Status,
}
pub(super) fn progress(
    record: &RecordDefinition,
    catalog: &Catalog,
    snapshot: &CollectionStateSnapshot,
) -> Progress {
    let values = record
        .objectives
        .iter()
        .map(|index| {
            if let Some(runtime) = &record.runtime {
                return runtime
                    .progress
                    .iter()
                    .find(|progress| progress.objective == *index)
                    .or_else(|| runtime.progress.first())
                    .map(|progress| {
                        snapshot
                            .values
                            .get(&(1, usize::from(progress.slot)))
                            .copied()
                            .unwrap_or(0)
                    });
            }
            let objective = catalog.objective_definition(*index)?;
            snapshot.evaluated_value(
                usize::from(objective.related_unlock_value_definition_index?),
                catalog,
            )
        })
        .collect::<Vec<_>>();
    let completed_objectives = record
        .objectives
        .iter()
        .zip(&values)
        .filter(|(index, value)| {
            value.is_some_and(|value| {
                catalog
                    .objective_definition(**index)
                    .is_some_and(|objective| objective_complete(objective, value))
            })
        })
        .count();
    let flag = record.completion_flag.and_then(|index| {
        if record.runtime.is_some() {
            let definition = catalog.unlock_flag_definition(usize::from(index))?;
            let slot = usize::from(definition.compact_slot?);
            (definition.bank() == 1).then(|| snapshot.flags.contains(&(1, slot)))
        } else {
            snapshot.evaluated_flag(usize::from(index), catalog)
        }
    });
    let all_stages = record.interval_count > 0
        && record
            .redeemed_intervals
            .and_then(|index| {
                catalog
                    .unlock_value_definition(usize::from(index))
                    .and_then(|definition| {
                        if record.runtime.is_some() {
                            let slot = usize::from(definition.compact_slot?);
                            (definition.bank() == 1)
                                .then(|| snapshot.values.get(&(1, slot)).copied().unwrap_or(0))
                        } else {
                            snapshot.value(usize::from(index), definition)
                        }
                    })
            })
            .is_some_and(|count| count >= 0 && count as usize == record.interval_count);
    let status = if all_stages
        || ((record.redeemed_intervals.is_none() || record.interval_count == 0)
            && flag == Some(true))
    {
        Status::Completed
    } else if !record.objectives.is_empty() && completed_objectives == record.objectives.len() {
        Status::ObjectivesComplete
    } else if flag == Some(false) || values.iter().any(Option::is_some) {
        Status::NotCompleted
    } else {
        Status::Unresolved
    };
    Progress {
        values,
        completed_objectives,
        status,
    }
}
