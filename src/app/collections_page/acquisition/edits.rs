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
    OBJECTIVE_INSTRUCTION, VALUE_INSTRUCTION, acquisition_status,
    expression::evaluate_expression_with,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CollectionStateEdit {
    Flag { definition_index: usize, set: bool },
    Value { definition_index: usize, value: i32 },
}

pub(in crate::app::collections_page) fn draw_collection_acquisition_action(
    ui: &mut egui::Ui,
    document: &mut Value,
    definition: &CollectibleDef,
    snapshot: &CollectionStateSnapshot,
    catalog: &Catalog,
    state: &mut UiState,
) -> bool {
    let current = acquisition_status(definition, snapshot, catalog).state;
    let desired = match current {
        AcquisitionState::Acquired => false,
        AcquisitionState::Missing => true,
        AcquisitionState::NoRule | AcquisitionState::Unknown => return false,
    };
    let edits = collection_state_edits(definition, snapshot, catalog, desired);
    let label = if desired {
        "Set acquired"
    } else {
        "Set missing"
    };
    let response = ui
        .add_enabled(edits.is_some(), egui::Button::new(label))
        .on_hover_text(if edits.is_some() {
            "Update the referenced Sunrise state and verify the acquisition condition"
        } else {
            "No reversible Sunrise state edit can produce this acquisition state"
        });
    let mut changed = false;
    if response.clicked() {
        let result = edits.map_or_else(
            || Err("No reversible Sunrise state edit is available".to_owned()),
            |edits| apply_collection_state_edits(document, definition, catalog, desired, &edits),
        );
        match result {
            Ok(()) => {
                let result = if desired { "Acquired" } else { "Missing" };
                state.mutation_feedback =
                    Some((false, format!("Acquisition state set to {result}")));
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

    let mut references = Vec::<(bool, usize)>::new();
    for token in &condition.tokens {
        let reference = match token.kind {
            FLAG_INSTRUCTION => Some((true, token.operand as usize)),
            VALUE_INSTRUCTION => Some((false, token.operand as usize)),
            OBJECTIVE_INSTRUCTION => catalog
                .objective_definition(token.operand as usize)
                .and_then(|objective| objective.related_unlock_value_definition_index)
                .map(|index| (false, usize::from(index))),
            _ => None,
        };
        if let Some(reference) = reference
            && !references.contains(&reference)
        {
            references.push(reference);
        }
    }
    if references.is_empty() || references.len() > 4 {
        return None;
    }

    let mut value_candidates = vec![0, 1];
    for token in &condition.tokens {
        if token.kind == LITERAL_INSTRUCTION {
            let literal = token.operand as i32;
            value_candidates.extend([
                literal,
                literal.saturating_sub(1),
                literal.saturating_add(1),
            ]);
        }
        if token.kind == OBJECTIVE_INSTRUCTION
            && let Some(objective) = catalog.objective_definition(token.operand as usize)
        {
            value_candidates.extend([
                objective.completion_value,
                objective.completion_value.saturating_sub(1),
                objective.completion_value.saturating_add(1),
            ]);
        }
    }
    value_candidates.sort_unstable();
    value_candidates.dedup();

    let mut options = Vec::<Vec<CollectionStateEdit>>::new();
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
        } else {
            let definition = catalog.unlock_value_definition(definition_index)?;
            if definition.compact_slot.is_some() && !matches!(definition.bank(), 1 | 2) {
                return None;
            }
            let mut candidates = value_candidates.clone();
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
    }
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
            |index| objective_completion_with_edits(index, candidate, snapshot, catalog),
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

fn objective_completion_with_edits(
    index: usize,
    edits: &[CollectionStateEdit],
    snapshot: &CollectionStateSnapshot,
    catalog: &Catalog,
) -> Option<bool> {
    let objective = catalog.objective_definition(index)?;
    let definition_index = usize::from(objective.related_unlock_value_definition_index?);
    let definition = catalog.unlock_value_definition(definition_index)?;
    let current = edits
        .iter()
        .find_map(|edit| match edit {
            CollectionStateEdit::Value {
                definition_index: edit_index,
                value,
            } if *edit_index == definition_index => Some(*value),
            _ => None,
        })
        .or_else(|| snapshot.value(definition_index, definition))?;
    Some(if objective.is_counting_downward {
        current <= objective.completion_value
    } else {
        current >= objective.completion_value
    })
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
