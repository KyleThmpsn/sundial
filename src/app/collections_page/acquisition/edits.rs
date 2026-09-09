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
        AcquisitionState::Missing => true,
        AcquisitionState::Unknown => return false,
    };
    let edit_available =
        collectible_acquisition_edit_available(definition, snapshot, catalog, desired);
    let label = if desired {
        "Set acquired"
    } else {
        "Set missing"
    };
    let response = ui
        .add_enabled(edit_available, egui::Button::new(label))
        .on_hover_text(if edit_available {
            "Update the referenced Sunrise state and verify the local condition. Shared state can affect other content, and native progression may reassert some flags."
        } else {
            "No supported edit was found within Sundial's bounded acquisition search"
        });
    let mut changed = false;
    if response.clicked() {
        let result =
            set_collectible_acquisition_state(document, definition, snapshot, catalog, desired);
        match result {
            Ok(()) => {
                let result = if desired { "Acquired" } else { "Missing" };
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
            ui.label(egui::RichText::new(message).weak());
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
    collection_state_edits(definition, snapshot, catalog, desired).is_some()
}

pub(in crate::app) fn set_collectible_acquisition_state(
    document: &mut Value,
    definition: &CollectibleDef,
    snapshot: &CollectionStateSnapshot,
    catalog: &Catalog,
    desired: bool,
) -> Result<(), String> {
    let edits =
        collection_state_edits(definition, snapshot, catalog, desired).ok_or_else(|| {
            "No supported edit was found within Sundial's bounded acquisition search".to_owned()
        })?;
    apply_collection_state_edits(document, definition, catalog, desired, &edits)
}

fn collection_state_edits(
    definition: &CollectibleDef,
    snapshot: &CollectionStateSnapshot,
    catalog: &Catalog,
    desired: bool,
) -> Option<Vec<CollectionStateEdit>> {
    let condition = definition
        .conditions
        .iter()
        .find(|condition| condition.field == ACQUISITION_CONDITION_FIELD)?;
    if definition
        .conditions
        .iter()
        .filter(|condition| condition.field == ACQUISITION_CONDITION_FIELD)
        .count()
        != 1
    {
        return None;
    }

    let references = collection_state_references(&condition.tokens, catalog);
    if references.is_empty() || references.len() > 4 {
        return None;
    }

    let value_candidates = collection_value_candidates(&condition.tokens, catalog);
    let options = collection_edit_options(references, &value_candidates, snapshot, catalog)?;
    if options
        .iter()
        .map(Vec::len)
        .try_fold(1_usize, usize::checked_mul)
        .is_none_or(|count| count > 256)
    {
        return None;
    }

    let mut best = None::<Vec<CollectionStateEdit>>;
    enumerate_collection_edits(&options, 0, &mut Vec::new(), &mut |candidate| {
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
    });
    best
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
            AcquisitionState::Missing
        }
    {
        return Err("The acquisition condition did not reach the requested state".into());
    }
    *document = candidate;
    Ok(())
}
