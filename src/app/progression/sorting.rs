//! Table ordering with missing values last and expensive keys computed once per row.
#[cfg(test)]
use super::{
    Catalog, ProgressionDefinition, UnlockDefinition,
    hierarchy::{progression_display_name, sort_by_optional_cached_key},
    override_meaning, progression_target,
    state::{ProgressionDisplayRow, TableSort},
};

#[cfg(test)]
pub(super) fn sort_progression_rows(
    rows: &mut [ProgressionDisplayRow],
    catalog: &Catalog,
    sort: TableSort,
) {
    let definition =
        |row: &ProgressionDisplayRow| catalog.progression_definition(row.definition_index);
    match sort.column {
        0 => sort_by_optional_cached_key(rows, sort.descending, |row| Some(row.definition_index)),
        1 => sort_by_optional_cached_key(rows, sort.descending, |row| {
            definition(row).map(|definition| definition.hash)
        }),
        2 => sort_by_optional_cached_key(rows, sort.descending, |row| {
            definition(row)
                .and_then(progression_display_name)
                .map(|name| name.to_lowercase())
        }),
        3 => sort_by_optional_cached_key(rows, sort.descending, |row| {
            definition(row).and_then(|definition| definition.scope_slot)
        }),
        4 | 6..=7 => {
            let lane = if sort.column == 4 { 0 } else { sort.column - 5 };
            sort_by_optional_cached_key(rows, sort.descending, |row| {
                row.lanes.map(|lanes| lanes[lane])
            });
        }
        5 => sort_by_optional_cached_key(rows, sort.descending, |row| {
            definition(row).and_then(progression_target)
        }),
        _ => {}
    }
}

#[cfg(test)]
pub(super) fn sort_progression_definitions(rows: &mut [&ProgressionDefinition], sort: TableSort) {
    match sort.column {
        0 => sort_by_optional_cached_key(rows, sort.descending, |row| Some(row.definition_index)),
        1 => sort_by_optional_cached_key(rows, sort.descending, |row| Some(row.hash)),
        2 => sort_by_optional_cached_key(rows, sort.descending, |row| {
            progression_display_name(row).map(|name| name.to_lowercase())
        }),
        _ => {}
    }
}

#[cfg(test)]
pub(super) fn sort_overrides<'a, T, V: Ord>(
    rows: &mut [T],
    sort: TableSort,
    fields: impl Fn(&T) -> (usize, V),
    definition: impl Fn(usize) -> Option<&'a UnlockDefinition>,
) {
    match sort.column {
        0 => sort_by_optional_cached_key(rows, sort.descending, |row| Some(fields(row).0)),
        1 => sort_by_optional_cached_key(rows, sort.descending, |row| {
            definition(fields(row).0).map(|definition| definition.hash)
        }),
        2 => sort_by_optional_cached_key(rows, sort.descending, |row| Some(fields(row).1)),
        3 => sort_by_optional_cached_key(rows, sort.descending, |row| {
            definition(fields(row).0).map(|definition| override_meaning(definition).to_lowercase())
        }),
        _ => {}
    }
}

#[cfg(test)]
mod tests;

pub(super) use super::hierarchy::sort_by_optional_cached_key as by_key;
