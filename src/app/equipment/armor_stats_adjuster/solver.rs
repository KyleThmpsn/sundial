//! Bounded armor-stat search and solution ranking.

use super::*;

pub(super) fn solve(input: &LoadoutInput, targets: [u16; 6]) -> Solution {
    let mut states = vec![SearchState {
        totals: [0; 6],
        plans: Vec::with_capacity(input.candidates.len()),
        swaps: 0,
        plug_changes: 0,
        masterworks: 0,
        exotics: 0,
    }];
    for candidates in &input.candidates {
        let plans = slot_plans(candidates, targets);
        let mut next = HashMap::<[u16; 6], SearchState>::new();
        for state in &states {
            for plan in &plans {
                let candidate = &candidates[plan.candidate_index];
                let exotics = state.exotics + usize::from(candidate.exotic);
                if exotics > 1 {
                    continue;
                }
                let mut candidate = state.clone();
                for (total, value) in candidate.totals.iter_mut().zip(plan.totals) {
                    *total = total.saturating_add(value);
                }
                candidate.plans.push(plan.clone());
                candidate.swaps += usize::from(plan.candidate_index != 0);
                candidate.plug_changes += plan.plug_changes;
                candidate.masterworks += plan.masterworks;
                candidate.exotics = exotics;
                let key = solver_key(candidate.totals, targets);
                match next.entry(key) {
                    std::collections::hash_map::Entry::Vacant(entry) => {
                        entry.insert(candidate);
                    }
                    std::collections::hash_map::Entry::Occupied(mut entry) => {
                        if partial_cmp(&candidate, entry.get(), targets) == Ordering::Less {
                            entry.insert(candidate);
                        }
                    }
                }
            }
        }
        if next.len() > MAX_SOLVER_STATES {
            next = prune_search_map(next, targets);
        }
        states = next.into_values().collect();
    }

    let best = states
        .into_iter()
        .min_by(|left, right| final_cmp(left, right, targets))
        .unwrap_or(SearchState {
            totals: [0; 6],
            plans: Vec::new(),
            swaps: 0,
            plug_changes: 0,
            masterworks: 0,
            exotics: 0,
        });
    solution_from_search(input, targets, best)
}

pub(super) fn slot_plans(candidates: &[ArmorCandidate], targets: [u16; 6]) -> Vec<PiecePlan> {
    let mut by_result = HashMap::<([u16; 6], bool), PiecePlan>::new();
    for (candidate_index, candidate) in candidates.iter().enumerate() {
        for plan in piece_plans(candidate, candidate_index, targets) {
            let key = (solver_key(plan.totals, targets), candidate.exotic);
            match by_result.entry(key) {
                std::collections::hash_map::Entry::Vacant(entry) => {
                    entry.insert(plan);
                }
                std::collections::hash_map::Entry::Occupied(mut entry) => {
                    if piece_plan_cmp(&plan, entry.get(), targets) == Ordering::Less {
                        entry.insert(plan);
                    }
                }
            }
        }
    }
    let mut plans = by_result.into_values().collect::<Vec<_>>();
    plans.sort_by(|left, right| piece_plan_cmp(left, right, targets));
    plans.truncate(MAX_SLOT_PLANS);
    plans
}

pub(super) fn piece_plans(
    candidate: &ArmorCandidate,
    candidate_index: usize,
    targets: [u16; 6],
) -> Vec<PiecePlan> {
    let mut states = vec![PieceSearchState {
        totals: candidate.fixed_totals,
        selections: Vec::with_capacity(candidate.sockets.len()),
        plug_changes: 0,
        masterworks: 0,
    }];
    for socket in &candidate.sockets {
        let choices = choices_for_targets(socket, targets);
        let mut next = HashMap::<[u16; 6], PieceSearchState>::new();
        for state in &states {
            for choice in &choices {
                let mut next_state = state.clone();
                for (total, value) in next_state.totals.iter_mut().zip(choice.values) {
                    *total = total.saturating_add(value);
                }
                next_state.selections.push(choice.hash);
                if choice.hash != socket.current {
                    match socket.kind {
                        SocketKind::Stat => next_state.plug_changes += 1,
                        SocketKind::Masterwork => next_state.masterworks += 1,
                    }
                }
                let key = solver_key(next_state.totals, targets);
                match next.entry(key) {
                    std::collections::hash_map::Entry::Vacant(entry) => {
                        entry.insert(next_state);
                    }
                    std::collections::hash_map::Entry::Occupied(mut entry) => {
                        if piece_state_cmp(&next_state, entry.get(), targets) == Ordering::Less {
                            entry.insert(next_state);
                        }
                    }
                }
            }
        }
        let mut next = next.into_values().collect::<Vec<_>>();
        next.sort_by(|left, right| piece_state_cmp(left, right, targets));
        next.truncate(MAX_PIECE_STATES);
        states = next;
    }
    states
        .into_iter()
        .map(|state| PiecePlan {
            candidate_index,
            totals: state.totals,
            selections: state.selections,
            plug_changes: state.plug_changes,
            masterworks: state.masterworks,
        })
        .collect()
}

pub(super) fn piece_state_cmp(
    left: &PieceSearchState,
    right: &PieceSearchState,
    targets: [u16; 6],
) -> Ordering {
    score(left.totals, targets)
        .cmp(&score(right.totals, targets))
        .then_with(|| waste_above_cap(left.totals).cmp(&waste_above_cap(right.totals)))
        .then_with(|| {
            (left.plug_changes + left.masterworks).cmp(&(right.plug_changes + right.masterworks))
        })
        .then_with(|| left.plug_changes.cmp(&right.plug_changes))
        .then_with(|| left.selections.cmp(&right.selections))
}

pub(super) fn piece_plan_cmp(left: &PiecePlan, right: &PiecePlan, targets: [u16; 6]) -> Ordering {
    score(left.totals, targets)
        .cmp(&score(right.totals, targets))
        .then_with(|| waste_above_cap(left.totals).cmp(&waste_above_cap(right.totals)))
        .then_with(|| (left.candidate_index != 0).cmp(&(right.candidate_index != 0)))
        .then_with(|| {
            (left.plug_changes + left.masterworks).cmp(&(right.plug_changes + right.masterworks))
        })
        .then_with(|| left.plug_changes.cmp(&right.plug_changes))
        .then_with(|| left.candidate_index.cmp(&right.candidate_index))
        .then_with(|| left.selections.cmp(&right.selections))
}

pub(super) fn choices_for_targets(socket: &MutableSocket, targets: [u16; 6]) -> Vec<SocketChoice> {
    let mut groups = HashMap::<[i32; 6], Vec<SocketChoice>>::new();
    for choice in &socket.choices {
        let key = std::array::from_fn(|index| {
            if targets[index] == 0 {
                0
            } else {
                choice.values[index]
            }
        });
        groups.entry(key).or_default().push(*choice);
    }

    let ignored_total = |choice: &SocketChoice| {
        choice
            .values
            .into_iter()
            .zip(targets)
            .filter(|(_, target)| *target == 0)
            .map(|(value, _)| value)
            .sum::<i32>()
    };
    let mut choices = Vec::new();
    for group in groups.into_values() {
        let mut keep = Vec::new();
        let mut retain = |candidate: Option<&SocketChoice>| {
            if let Some(candidate) = candidate
                && !keep.contains(candidate)
            {
                keep.push(*candidate);
            }
        };
        retain(group.iter().find(|choice| choice.hash == socket.current));
        retain(
            group
                .iter()
                .min_by_key(|choice| (ignored_total(choice), choice.hash)),
        );
        retain(
            group
                .iter()
                .max_by_key(|choice| (ignored_total(choice), Reverse(choice.hash))),
        );
        choices.extend(keep);
    }
    choices.sort_by_key(|choice| (choice.hash != socket.current, choice.values, choice.hash));
    choices
}

pub(super) fn prune_search_map(
    states: HashMap<[u16; 6], SearchState>,
    targets: [u16; 6],
) -> HashMap<[u16; 6], SearchState> {
    let mut states = states.into_values().collect::<Vec<_>>();
    states.sort_by(|left, right| partial_cmp(left, right, targets));
    states.truncate(MAX_SOLVER_STATES);
    states
        .into_iter()
        .map(|state| (solver_key(state.totals, targets), state))
        .collect()
}

pub(super) fn solution_from_search(
    input: &LoadoutInput,
    targets: [u16; 6],
    search: SearchState,
) -> Solution {
    let mut selections = Vec::with_capacity(search.plans.len());
    let mut swaps = Vec::new();
    let mut assignments = Vec::new();
    for (piece_index, plan) in search.plans.iter().enumerate() {
        let candidate = &input.candidates[piece_index][plan.candidate_index];
        selections.push(PieceSelection {
            candidate_index: plan.candidate_index,
            projected_totals: capped_totals(plan.totals),
        });
        if let ArmorOrigin::Inventory {
            instance_soid,
            definition_hash,
        } = candidate.origin
        {
            swaps.push(ArmorSwap {
                piece_index,
                instance_soid,
                definition_hash,
            });
        }
        assignments.extend(candidate.sockets.iter().zip(&plan.selections).filter_map(
            |(socket, selected)| {
                (*selected != socket.current).then_some(SocketAssignment {
                    piece_index,
                    socket_index: socket.socket_index,
                    previous: socket.current,
                    selected: *selected,
                    kind: socket.kind,
                })
            },
        ));
    }
    let projected_totals = capped_totals(search.totals);
    let shortfalls = std::array::from_fn(|index| {
        if targets[index] == 0 {
            0
        } else {
            targets[index].saturating_sub(projected_totals[index])
        }
    });
    Solution {
        projected_totals,
        selections,
        swaps,
        assignments,
        shortfalls,
        exact: shortfalls.iter().all(|shortfall| *shortfall == 0),
    }
}

pub(super) fn partial_cmp(left: &SearchState, right: &SearchState, targets: [u16; 6]) -> Ordering {
    score(left.totals, targets)
        .cmp(&score(right.totals, targets))
        .then_with(|| waste_above_cap(left.totals).cmp(&waste_above_cap(right.totals)))
        .then_with(|| left.swaps.cmp(&right.swaps))
        .then_with(|| {
            (left.plug_changes + left.masterworks).cmp(&(right.plug_changes + right.masterworks))
        })
        .then_with(|| left.plug_changes.cmp(&right.plug_changes))
        .then_with(|| {
            target_excess(left.totals, targets).cmp(&target_excess(right.totals, targets))
        })
        .then_with(|| total_value(right.totals, targets).cmp(&total_value(left.totals, targets)))
        .then_with(|| plan_tie_key(left).cmp(&plan_tie_key(right)))
}

pub(super) fn final_cmp(left: &SearchState, right: &SearchState, targets: [u16; 6]) -> Ordering {
    let left_score = score(left.totals, targets);
    let right_score = score(right.totals, targets);
    left_score
        .cmp(&right_score)
        .then_with(|| waste_above_cap(left.totals).cmp(&waste_above_cap(right.totals)))
        .then_with(|| left.swaps.cmp(&right.swaps))
        .then_with(|| {
            (left.plug_changes + left.masterworks).cmp(&(right.plug_changes + right.masterworks))
        })
        .then_with(|| left.plug_changes.cmp(&right.plug_changes))
        .then_with(|| {
            target_excess(left.totals, targets).cmp(&target_excess(right.totals, targets))
        })
        .then_with(|| total_value(right.totals, targets).cmp(&total_value(left.totals, targets)))
        .then_with(|| plan_tie_key(left).cmp(&plan_tie_key(right)))
}

pub(super) fn plan_tie_key(state: &SearchState) -> Vec<(usize, Vec<Option<u64>>)> {
    state
        .plans
        .iter()
        .map(|plan| (plan.candidate_index, plan.selections.clone()))
        .collect()
}

pub(super) fn score(totals: [i32; 6], targets: [u16; 6]) -> (u32, u16) {
    let totals = capped_totals(totals);
    let mut shortfall = 0_u32;
    let mut largest = 0_u16;
    for index in 0..6 {
        if targets[index] == 0 {
            continue;
        }
        let missing = targets[index].saturating_sub(totals[index]);
        shortfall = shortfall.saturating_add(u32::from(missing));
        largest = largest.max(missing);
    }
    (shortfall, largest)
}

pub(super) fn target_excess(totals: [i32; 6], targets: [u16; 6]) -> u32 {
    capped_totals(totals)
        .into_iter()
        .zip(targets)
        .filter(|(_, target)| *target > 0)
        .map(|(total, target)| u32::from(total.saturating_sub(target)))
        .sum()
}

pub(super) fn total_value(totals: [i32; 6], targets: [u16; 6]) -> u32 {
    capped_totals(totals)
        .into_iter()
        .zip(targets)
        .filter(|(_, target)| *target == 0)
        .map(|(total, _)| u32::from(total))
        .sum()
}

pub(super) fn waste_above_cap(totals: [i32; 6]) -> u32 {
    totals
        .into_iter()
        .map(|value| value.saturating_sub(i32::from(armor_stat_allocation::TARGET_MAX)))
        .filter_map(|value| u32::try_from(value).ok())
        .sum()
}

pub(super) fn capped_totals(totals: [i32; 6]) -> [u16; 6] {
    totals.map(|value| {
        u16::try_from(value.clamp(0, i32::from(armor_stat_allocation::TARGET_MAX)))
            .unwrap_or(armor_stat_allocation::TARGET_MAX)
    })
}

pub(super) fn cap_u16_totals(totals: [u16; 6]) -> [u16; 6] {
    totals.map(|value| value.min(armor_stat_allocation::TARGET_MAX))
}

pub(super) fn solver_key(totals: [i32; 6], targets: [u16; 6]) -> [u16; 6] {
    let totals = capped_totals(totals);
    std::array::from_fn(|index| {
        if targets[index] == 0 {
            0
        } else {
            totals[index]
        }
    })
}
