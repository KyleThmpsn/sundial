//! Atomic application of a reviewed armor-stat solution.

use crate::app::account_workspace as account;

use super::*;

pub(super) fn apply_preview(app: &mut SundialApp, state: &mut State, character_index: usize) {
    let Some(input) = state.input.as_ref() else {
        state.feedback = Some(Feedback {
            text: "Equipped armor is unavailable".to_owned(),
            detail: None,
            is_error: true,
        });
        return;
    };
    let Some(solution) = state.preview.clone() else {
        return;
    };
    if solution.assignments.is_empty() && solution.swaps.is_empty() {
        return;
    }

    let mut updated = app.document.clone();
    let result = (|| {
        for swap in &solution.swaps {
            let current = input
                .pieces
                .get(swap.piece_index)
                .ok_or("The armor preview is stale")?;
            if current.locked {
                return Err(format!("{} is locked", current.label));
            }
            let candidate = selected_candidate(input, &solution, swap.piece_index)
                .ok_or("The selected inventory armor is unavailable")?;
            let item = candidate
                .piece
                .item
                .as_ref()
                .ok_or("An armor definition is unavailable")?;
            if u32::try_from(item.hash).ok() != Some(swap.definition_hash) {
                return Err(
                    "The selected inventory armor changed before it could be equipped".to_owned(),
                );
            }
            let location = account::character_inventory(&updated, character_index)
                .map_err(|error| error.to_string())?
                .and_then(|items| {
                    items
                        .into_iter()
                        .find(|snapshot| snapshot.instance_soid == swap.instance_soid)
                        .map(|snapshot| snapshot.location)
                })
                .ok_or("The selected inventory armor no longer exists")?;
            equip_inventory_item(
                &mut updated,
                location,
                current.slot,
                item,
                app.preferences.experimental_cross_class_subclasses,
            )?;
        }
        for assignment in &solution.assignments {
            let candidate = selected_candidate(input, &solution, assignment.piece_index)
                .ok_or("The armor preview is stale")?;
            let piece = &candidate.piece;
            if piece.locked || piece.issue.is_some() {
                return Err(format!("{} is no longer editable", piece.label));
            }
            let item = piece
                .item
                .as_ref()
                .ok_or("An armor definition is unavailable")?;
            account::set_equipment_item_plug(
                &mut updated,
                character_index,
                piece.slot,
                assignment.socket_index,
                &item.default_plugs,
                assignment.selected,
            )?;
        }
        settings::validate_workspace_document(&updated)
            .map_err(|error| format!("Adjusted armor did not pass validation: {error}"))?;
        Ok::<(), String>(())
    })();

    match result {
        Ok(()) => {
            let plug_count = solution
                .assignments
                .iter()
                .filter(|assignment| assignment.kind == SocketKind::Stat)
                .count();
            let masterwork_count = solution
                .assignments
                .iter()
                .filter(|assignment| assignment.kind == SocketKind::Masterwork)
                .count();
            let swap_count = solution.swaps.len();
            let piece_count = changed_piece_count(&solution);
            let (summary, detail) = if solution.exact {
                (
                    "Targets met".to_owned(),
                    "Every selected goal was met".to_owned(),
                )
            } else {
                let missed = solution
                    .shortfalls
                    .iter()
                    .filter(|shortfall| **shortfall > 0)
                    .count();
                (
                    format!("Closest match · {missed} goals short"),
                    format!("Closest match · {}", format_shortfalls(solution.shortfalls)),
                )
            };
            app.document = updated;
            app.dirty = true;
            app.set_status(
                format!(
                    "Armor adjusted. Pieces: {piece_count}, swaps: {swap_count}, masterworks: {masterwork_count}, stat plugs: {plug_count}. Click Save to write it"
                ),
                false,
            );
            state.feedback = Some(Feedback {
                text: format!("Armor adjusted · {summary}"),
                detail: Some(detail),
                is_error: false,
            });
            state.source_key = None;
            state.input = None;
            state.preview = None;
            state.preview_task = None;
            state.preview_due_at = None;
            state.preserve_feedback_once = true;
        }
        Err(error) => {
            app.set_status(format!("Armor stats not adjusted: {error}"), true);
            state.feedback = Some(Feedback {
                text: error,
                detail: None,
                is_error: true,
            });
        }
    }
}

pub(super) fn selected_candidate<'a>(
    input: &'a LoadoutInput,
    solution: &Solution,
    piece_index: usize,
) -> Option<&'a ArmorCandidate> {
    let candidate_index = solution
        .selections
        .get(piece_index)
        .map_or(0, |selection| selection.candidate_index);
    input.candidates.get(piece_index)?.get(candidate_index)
}

pub(super) fn changed_piece_count(solution: &Solution) -> usize {
    let mut pieces = solution
        .assignments
        .iter()
        .map(|assignment| assignment.piece_index)
        .chain(solution.swaps.iter().map(|swap| swap.piece_index))
        .collect::<Vec<_>>();
    pieces.sort_unstable();
    pieces.dedup();
    pieces.len()
}

pub(super) fn format_shortfalls(shortfalls: [u16; 6]) -> String {
    armor_stat_allocation::STAT_NAMES
        .into_iter()
        .zip(shortfalls)
        .filter_map(|(name, shortfall)| {
            (shortfall > 0).then(|| format!("{name} {shortfall} short"))
        })
        .collect::<Vec<_>>()
        .join(" · ")
}

pub(super) fn format_totals(totals: [u16; 6]) -> String {
    armor_stat_allocation::STAT_NAMES
        .into_iter()
        .zip(totals)
        .map(|(name, total)| format!("{total} {name}"))
        .collect::<Vec<_>>()
        .join(" · ")
}

pub(super) fn plug_name(catalog: &Catalog, hash: Option<u64>) -> String {
    hash.and_then(|hash| catalog.display_name(hash).map(str::to_owned))
        .unwrap_or_else(|| "Empty".to_owned())
}
