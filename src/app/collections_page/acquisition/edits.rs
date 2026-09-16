use eframe::egui;
use serde_json::Value;

use crate::catalog::{Catalog, CollectibleDef};

use super::{
    super::{
        super::progression::{
            CollectionStateSnapshot, collection_state_snapshot, set_collection_flag,
            set_collection_value,
        },
        UiState,
    },
    ACQUISITION_CONDITION_FIELD, AcquisitionState, FLAG_INSTRUCTION, LITERAL_INSTRUCTION,
    VALUE_INSTRUCTION, acquisition_status,
    expression::evaluate_expression_with,
    for_each_expression_token,
};
mod solver;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CollectionStateEdit {
    Flag { definition_index: usize, set: bool },
    Value { definition_index: usize, value: i32 },
}

fn collection_state_references(
    tokens: &[crate::catalog::CollectionConditionTokenDef],
    catalog: &Catalog,
) -> Vec<(bool, usize)> {
    let mut references = Vec::new();
    for_each_expression_token(tokens, catalog, |token| {
        let reference = match token.kind {
            FLAG_INSTRUCTION => Some((true, token.operand as usize)),
            VALUE_INSTRUCTION => Some((false, token.operand as usize)),
            _ => None,
        };
        if let Some(reference) = reference
            && !references.contains(&reference)
        {
            references.push(reference);
        }
    });
    references
}

fn collection_value_candidates(
    tokens: &[crate::catalog::CollectionConditionTokenDef],
    catalog: &Catalog,
) -> Vec<i32> {
    let mut candidates = vec![0, 1];
    for_each_expression_token(tokens, catalog, |token| {
        if token.kind == LITERAL_INSTRUCTION {
            let literal = token.operand as i32;
            candidates.extend([
                literal,
                literal.saturating_sub(1),
                literal.saturating_add(1),
            ]);
        }
    });
    candidates.sort_unstable();
    candidates.dedup();
    candidates
}

fn collection_edit_options(
    references: Vec<(bool, usize)>,
    value_candidates: &[i32],
    snapshot: &CollectionStateSnapshot,
    catalog: &Catalog,
) -> Option<Vec<Vec<CollectionStateEdit>>> {
    let mut options = Vec::new();
    for (flag, definition_index) in references {
        if flag {
            let definition = catalog.unlock_flag_definition(definition_index)?;
            if definition.compact_slot.is_some() && !matches!(definition.bank(), 1 | 2 | 3 | 6) {
                return None;
            }
            options.push(
                [false, true]
                    .into_iter()
                    .map(|set| CollectionStateEdit::Flag {
                        definition_index,
                        set,
                    })
                    .collect(),
            );
            continue;
        }
        let definition = catalog.unlock_value_definition(definition_index)?;
        if definition.compact_slot.is_some() && !matches!(definition.bank(), 1 | 2) {
            return None;
        }
        let mut candidates = value_candidates.to_vec();
        if let Some(current) = snapshot.value(definition_index, definition) {
            candidates.push(current);
        }
        candidates.sort_unstable();
        candidates.dedup();
        options.push(
            candidates
                .into_iter()
                .map(|value| CollectionStateEdit::Value {
                    definition_index,
                    value,
                })
                .collect(),
        );
    }
    Some(options)
}

pub(in crate::app::collections_page) fn draw_collection_acquisition_action(
    ui: &mut egui::Ui,
    document: &mut Value,
    definition: &CollectibleDef,
    snapshot: &CollectionStateSnapshot,
    catalog: &Catalog,
    state: &mut UiState,
) -> bool {
    if state.read_only {
        return false;
    }
    let current = acquisition_status(definition, snapshot, catalog).state;
    let desired = match current {
        AcquisitionState::Acquired => false,
        AcquisitionState::NotAcquired => true,
        AcquisitionState::Unknown => {
            let reason = collection_state_edits(definition, snapshot, catalog, true)
                .err()
                .unwrap_or_else(|| "Current acquisition state is unresolved".into());
            ui.add_enabled(false, egui::Button::new("Unavailable"))
                .on_disabled_hover_text(reason);
            return false;
        }
    };
    let edit_available =
        collectible_acquisition_edit_available(definition, snapshot, catalog, desired);
    let label = if desired {
        "Acquire"
    } else {
        "Mark Not Acquired"
    };
    let response = ui
        .add_enabled(edit_available, egui::Button::new(label))
        .on_hover_text(if edit_available {
            "Update the saved acquisition condition"
        } else {
            "The acquisition condition cannot be changed from saved account state"
        });
    let mut changed = false;
    if response.clicked() {
        let result =
            set_collectible_acquisition_state(document, definition, snapshot, catalog, desired);
        match result {
            Ok(()) => {
                let result = if desired { "Acquired" } else { "Not Acquired" };
                state.mutation_feedback =
                    Some((false, format!("Authored acquisition state set to {result}")));
                changed = true;
            }
            Err(error) => state.mutation_feedback = Some((true, error)),
        }
    }
    if let Some((error, message)) = &state.mutation_feedback {
        if *error {
            ui.colored_label(ui.visuals().error_fg_color, message);
        } else {
            ui.weak(message);
        }
    }
    changed
}

pub(in crate::app) fn collectible_acquisition_edit_available(
    definition: &CollectibleDef,
    snapshot: &CollectionStateSnapshot,
    catalog: &Catalog,
    desired: bool,
) -> bool {
    if snapshot.is_native()
        && let Some(season) = catalog.seasonal()
        && let Some(entry) = season
            .mods
            .iter()
            .find(|entry| entry.collectible_hash == definition.hash)
    {
        return !desired
            || snapshot
                .seasonal_experience(season)
                .is_ok_and(|experience| {
                    season
                        .unlock(
                            snapshot.artifact_mask(season, true),
                            entry.sale_index,
                            experience.points_earned,
                        )
                        .is_ok()
                });
    }
    collection_state_edits(definition, snapshot, catalog, desired).is_ok()
}

pub(in crate::app) fn set_collectible_acquisition_state(
    document: &mut Value,
    definition: &CollectibleDef,
    snapshot: &CollectionStateSnapshot,
    catalog: &Catalog,
    desired: bool,
) -> Result<(), String> {
    if snapshot.is_native()
        && let Some(entry) = catalog.seasonal().and_then(|season| {
            season
                .mods
                .iter()
                .find(|entry| entry.collectible_hash == definition.hash)
        })
    {
        crate::app::progression::seasonal::apply(
            document,
            catalog,
            crate::app::progression::seasonal::Edit::Mod {
                sale_index: entry.sale_index,
                owned: desired,
            },
        )?;
        return Ok(());
    }
    let edits = collection_state_edits(definition, snapshot, catalog, desired)?;
    apply_collection_state_edits(document, definition, catalog, desired, &edits)
}

fn collection_state_edits(
    definition: &CollectibleDef,
    snapshot: &CollectionStateSnapshot,
    catalog: &Catalog,
    desired: bool,
) -> Result<Vec<CollectionStateEdit>, String> {
    let condition = definition
        .conditions
        .iter()
        .find(|condition| condition.field == ACQUISITION_CONDITION_FIELD)
        .ok_or("No acquisition condition is stored for this item")?;
    if definition
        .conditions
        .iter()
        .filter(|condition| condition.field == ACQUISITION_CONDITION_FIELD)
        .count()
        != 1
    {
        return Err("Multiple acquisition conditions are stored for this item".into());
    }
    let mut unsupported = None;
    let complete = for_each_expression_token(&condition.tokens, catalog, |token| {
        if !super::expression::is_supported_instruction(token.kind) {
            unsupported = Some(token.kind);
        }
    });
    if !complete {
        return Err("The acquisition condition has a missing or cyclic shared expression".into());
    }
    if let Some(kind) = unsupported {
        return Err(format!("Condition instruction {kind} is not decoded"));
    }

    let references = collection_state_references(&condition.tokens, catalog);
    if references.is_empty() {
        return Err("The condition has no saved flag or counter to change".into());
    }
    if references.len() > 64 {
        return Err(format!(
            "The condition references {} inputs, exceeding the 64-input edit limit",
            references.len()
        ));
    }
    if let Some(edits) = solver::solve(&condition.tokens, snapshot, catalog, desired) {
        return Ok(edits);
    }
    for (flag, index) in &references {
        let definition = if *flag {
            catalog.unlock_flag_definition(*index)
        } else {
            catalog.unlock_value_definition(*index)
        }
        .ok_or_else(|| {
            format!(
                "{} definition #{index} is unavailable",
                if *flag { "Unlock" } else { "Counter" }
            )
        })?;
        let writable = if *flag {
            matches!(definition.bank(), 1 | 2 | 3 | 6)
        } else {
            matches!(definition.bank(), 1 | 2)
        };
        if definition.compact_slot.is_some() && !writable {
            return Err(format!(
                "{} #{index} uses bank {}, which has no saved edit path",
                if *flag { "Unlock" } else { "Counter" },
                definition.bank()
            ));
        }
    }
    let value_candidates = collection_value_candidates(&condition.tokens, catalog);
    let options = collection_edit_options(references, &value_candidates, snapshot, catalog)
        .ok_or("The condition references an unavailable saved value")?;
    let exhaustive = options
        .iter()
        .map(Vec::len)
        .try_fold(1_usize, usize::checked_mul)
        .is_some_and(|count| count <= 4096);

    let mut best = None::<Vec<CollectionStateEdit>>;
    let mut visit = |candidate: &[CollectionStateEdit]| {
        let result = evaluate_expression_with(
            &condition.tokens,
            catalog.shared_expression_pool(),
            |index| {
                candidate
                    .iter()
                    .find_map(|edit| match edit {
                        CollectionStateEdit::Flag {
                            definition_index,
                            set,
                        } if *definition_index == index => Some(*set),
                        _ => None,
                    })
                    .or_else(|| {
                        let definition = catalog.unlock_flag_definition(index)?;
                        snapshot.flag_value(index, definition)
                    })
            },
            |index| {
                candidate
                    .iter()
                    .find_map(|edit| match edit {
                        CollectionStateEdit::Value {
                            definition_index,
                            value,
                        } if *definition_index == index => Some(*value),
                        _ => None,
                    })
                    .or_else(|| {
                        let definition = catalog.unlock_value_definition(index)?;
                        snapshot.value(index, definition)
                    })
            },
        );
        if result != Some(desired) {
            return;
        }
        let changed = candidate
            .iter()
            .copied()
            .filter(|edit| collection_edit_changes_state(*edit, snapshot, catalog))
            .collect::<Vec<_>>();
        if changed.is_empty() {
            return;
        }
        if best
            .as_ref()
            .is_none_or(|current| changed.len() < current.len())
        {
            best = Some(changed);
        }
    };
    if exhaustive {
        enumerate_collection_edits(&options, 0, &mut Vec::new(), &mut visit);
    } else {
        // Try uniform states and one changed input without an exponential search.
        // Every proposed edit still passes the complete expression and persistence checks.
        for high in [false, true] {
            let candidate = options
                .iter()
                .filter_map(|values| if high { values.last() } else { values.first() })
                .copied()
                .collect::<Vec<_>>();
            visit(&candidate);
        }
        for values in &options {
            for value in values {
                visit(&[*value]);
            }
        }
    }
    best.ok_or_else(|| {
        if exhaustive {
            "No writable flag or counter values satisfied this condition".into()
        } else {
            format!(
                "No verified change found for this {}-input condition",
                options.len()
            )
        }
    })
}

fn enumerate_collection_edits(
    options: &[Vec<CollectionStateEdit>],
    index: usize,
    current: &mut Vec<CollectionStateEdit>,
    visit: &mut impl FnMut(&[CollectionStateEdit]),
) {
    if index == options.len() {
        visit(current);
        return;
    }
    for edit in &options[index] {
        current.push(*edit);
        enumerate_collection_edits(options, index + 1, current, visit);
        current.pop();
    }
}

fn collection_edit_changes_state(
    edit: CollectionStateEdit,
    snapshot: &CollectionStateSnapshot,
    catalog: &Catalog,
) -> bool {
    match edit {
        CollectionStateEdit::Flag {
            definition_index,
            set,
        } => {
            catalog
                .unlock_flag_definition(definition_index)
                .and_then(|definition| snapshot.flag_value(definition_index, definition))
                != Some(set)
        }
        CollectionStateEdit::Value {
            definition_index,
            value,
        } => {
            catalog
                .unlock_value_definition(definition_index)
                .and_then(|definition| snapshot.value(definition_index, definition))
                != Some(value)
        }
    }
}

fn apply_collection_state_edits(
    document: &mut Value,
    definition: &CollectibleDef,
    catalog: &Catalog,
    desired: bool,
    edits: &[CollectionStateEdit],
) -> Result<(), String> {
    let mut candidate = document.clone();
    for edit in edits {
        let applied = match *edit {
            CollectionStateEdit::Flag {
                definition_index,
                set,
            } => {
                let definition = catalog
                    .unlock_flag_definition(definition_index)
                    .ok_or_else(|| format!("Flag definition #{definition_index} is unavailable"))?;
                set_collection_flag(&mut candidate, definition_index, definition, set)
            }
            CollectionStateEdit::Value {
                definition_index,
                value,
            } => {
                let definition = catalog
                    .unlock_value_definition(definition_index)
                    .ok_or_else(|| {
                        format!("Value definition #{definition_index} is unavailable")
                    })?;
                set_collection_value(&mut candidate, definition_index, definition, value)
            }
        };
        if !applied {
            return Err("The referenced Sunrise state could not be updated".into());
        }
    }
    let snapshot = collection_state_snapshot(&candidate)
        .ok_or_else(|| "The updated progression settings are invalid".to_owned())?;
    if acquisition_status(definition, &snapshot, catalog).state
        != if desired {
            AcquisitionState::Acquired
        } else {
            AcquisitionState::NotAcquired
        }
    {
        return Err("The acquisition condition did not reach the requested state".into());
    }
    *document = candidate;
    Ok(())
}
