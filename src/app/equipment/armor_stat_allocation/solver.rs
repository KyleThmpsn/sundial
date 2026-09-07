use std::{
    cmp::Reverse,
    collections::{HashMap, hash_map::Entry},
};

use crate::{
    app::PlugSelectionMode,
    catalog::{Catalog, ItemDef},
};

use super::model::{
    AllocationGroup, Choice, CrossGroupChoice, CrossGroupFailure, CrossGroupSolution, GroupFailure,
    GroupSolution, Plan, STAT_NAMES, Solution, SolveFailure, group_index,
    mode_allows_cross_group_allocations,
};

pub(super) fn solve(
    catalog: &Catalog,
    item: &ItemDef,
    current: &[Option<u64>],
    mode: PlugSelectionMode,
    targets: [u16; 6],
) -> Result<Solution, SolveFailure> {
    if mode_allows_cross_group_allocations(mode) {
        return solve_cross_group(catalog, item, current, mode, targets);
    }

    let mut proposed = current.to_vec();
    let mut best_available = current.to_vec();
    let mut reason = None;
    let mut failed = false;

    for group in AllocationGroup::ALL {
        let indices = group.indices();
        let group_targets = indices.map(|index| targets[index]);
        if group_targets.iter().all(|target| *target == 0) {
            continue;
        }

        let fixed_values = fixed_group_values(catalog, item, current, group);
        let allocation_targets = remaining_targets(group_targets, fixed_values);
        if allocation_targets.iter().all(|target| *target == 0) {
            continue;
        }

        let sockets = allocation_socket_indices(catalog, item, current.len(), group);
        if sockets.is_empty() {
            failed = true;
            reason = Some(format!("{} is unavailable on this item", group.label()));
            continue;
        }

        let choice_sets = sockets
            .iter()
            .map(|socket_index| {
                allocation_choices_for_socket(
                    catalog,
                    item,
                    *socket_index,
                    current[*socket_index],
                    mode,
                    group,
                )
            })
            .collect::<Vec<_>>();
        if choice_sets.iter().any(Vec::is_empty) {
            failed = true;
            reason = Some(format!(
                "No {} plugs are available with {} safety",
                group.label().to_ascii_lowercase(),
                mode.label()
            ));
            continue;
        }

        match solve_group(&sockets, &choice_sets, current, allocation_targets) {
            Ok(solution) => {
                apply_assignments(&mut proposed, &solution.assignments);
                apply_assignments(&mut best_available, &solution.assignments);
            }
            Err(group_failure) => {
                failed = true;
                if let Some(best) = group_failure.best {
                    apply_assignments(&mut best_available, &best.assignments);
                }
                reason = reason.or(group_failure.reason);
            }
        }
    }

    if failed {
        let best_totals = selected_totals(catalog, item, &best_available);
        return Err(best_effort_failure(
            current,
            best_available,
            best_totals,
            reason,
        ));
    }

    let changed = proposed
        .iter()
        .zip(current)
        .filter(|(left, right)| left != right)
        .count();
    Ok(Solution {
        plugs: proposed,
        changed,
    })
}

fn solve_cross_group(
    catalog: &Catalog,
    item: &ItemDef,
    current: &[Option<u64>],
    mode: PlugSelectionMode,
    targets: [u16; 6],
) -> Result<Solution, SolveFailure> {
    if targets.iter().all(|target| *target == 0) {
        return Ok(Solution {
            plugs: current.to_vec(),
            changed: 0,
        });
    }

    let sockets = allocation_socket_indices_all(catalog, item, current.len());
    if sockets.is_empty() {
        return Err(best_effort_failure(
            current,
            current.to_vec(),
            selected_totals(catalog, item, current),
            Some("Stat allocation sockets are unavailable on this item".to_owned()),
        ));
    }

    let choice_sets = sockets
        .iter()
        .map(|socket_index| {
            cross_group_choices_for_socket(
                catalog,
                item,
                *socket_index,
                current[*socket_index],
                mode,
            )
        })
        .collect::<Vec<_>>();
    if choice_sets.iter().any(Vec::is_empty) {
        return Err(best_effort_failure(
            current,
            current.to_vec(),
            selected_totals(catalog, item, current),
            Some(format!(
                "No stat allocation plugs are available with {} safety",
                mode.label()
            )),
        ));
    }

    let fixed = fixed_non_allocation_values(catalog, item, current);
    match solve_cross_group_plan(&sockets, &choice_sets, current, fixed, targets) {
        Ok(solution) => {
            let mut plugs = current.to_vec();
            apply_assignments(&mut plugs, &solution.assignments);
            let changed = plugs
                .iter()
                .zip(current)
                .filter(|(left, right)| left != right)
                .count();
            Ok(Solution { plugs, changed })
        }
        Err(failure) => {
            let mut plugs = current.to_vec();
            let best_totals = if let Some(best) = failure.best {
                apply_assignments(&mut plugs, &best.assignments);
                best.values
            } else {
                selected_totals(catalog, item, current)
            };
            Err(best_effort_failure(current, plugs, best_totals, None))
        }
    }
}

pub(super) fn solve_cross_group_plan(
    sockets: &[usize],
    choice_sets: &[Vec<CrossGroupChoice>],
    current: &[Option<u64>],
    fixed: [u16; 6],
    targets: [u16; 6],
) -> Result<CrossGroupSolution, CrossGroupFailure> {
    let mut plans = HashMap::from([(
        [0_u16; 6],
        Plan {
            hashes: Vec::new(),
            changes: 0,
        },
    )]);

    for (socket_index, choices) in sockets.iter().copied().zip(choice_sets) {
        let mut next = HashMap::<[u16; 6], Plan>::new();
        for (total, plan) in &plans {
            for choice in choices {
                let combined =
                    std::array::from_fn(|index| total[index].saturating_add(choice.values[index]));
                let mut candidate = plan.clone();
                candidate.hashes.push(choice.hash);
                candidate.changes += usize::from(current[socket_index] != Some(choice.hash));

                match next.entry(combined) {
                    Entry::Vacant(entry) => {
                        entry.insert(candidate);
                    }
                    Entry::Occupied(mut entry) => {
                        if plan_tie_key(&candidate) < plan_tie_key(entry.get()) {
                            entry.insert(candidate);
                        }
                    }
                }
            }
        }
        plans = next;
    }

    let best_meeting = plans
        .iter()
        .filter(|(values, _)| meets(add_stat_values(fixed, **values), targets))
        .min_by_key(|(values, plan)| meeting_key(add_stat_values(fixed, **values), targets, plan));
    if let Some((values, plan)) = best_meeting {
        return Ok(cross_group_solution(
            sockets,
            add_stat_values(fixed, *values),
            plan,
        ));
    }

    let best = plans
        .iter()
        .min_by_key(|(values, plan)| failure_key(add_stat_values(fixed, **values), targets, plan))
        .map(|(values, plan)| cross_group_solution(sockets, add_stat_values(fixed, *values), plan));
    Err(CrossGroupFailure { best })
}

fn cross_group_solution(sockets: &[usize], values: [u16; 6], plan: &Plan) -> CrossGroupSolution {
    CrossGroupSolution {
        assignments: sockets
            .iter()
            .copied()
            .zip(plan.hashes.iter().copied())
            .collect(),
        values,
    }
}

fn add_stat_values(left: [u16; 6], right: [u16; 6]) -> [u16; 6] {
    std::array::from_fn(|index| left[index].saturating_add(right[index]))
}

pub(super) fn best_effort_failure(
    current: &[Option<u64>],
    plugs: Vec<Option<u64>>,
    best_totals: [u16; 6],
    reason: Option<String>,
) -> SolveFailure {
    let changed = plugs
        .iter()
        .zip(current)
        .filter(|(left, right)| left != right)
        .count();
    SolveFailure {
        plugs,
        changed,
        best_totals,
        reason,
    }
}

pub(super) fn solve_group(
    sockets: &[usize],
    choice_sets: &[Vec<Choice>],
    current: &[Option<u64>],
    targets: [u16; 3],
) -> Result<GroupSolution, GroupFailure> {
    let mut plans = HashMap::from([(
        [0_u16; 3],
        Plan {
            hashes: Vec::new(),
            changes: 0,
        },
    )]);

    for (socket_index, choices) in sockets.iter().copied().zip(choice_sets) {
        let mut next = HashMap::<[u16; 3], Plan>::new();
        for (total, plan) in &plans {
            for choice in choices {
                let combined = [
                    total[0].saturating_add(choice.values[0]),
                    total[1].saturating_add(choice.values[1]),
                    total[2].saturating_add(choice.values[2]),
                ];
                let mut candidate = plan.clone();
                candidate.hashes.push(choice.hash);
                candidate.changes += usize::from(current[socket_index] != Some(choice.hash));

                match next.entry(combined) {
                    Entry::Vacant(entry) => {
                        entry.insert(candidate);
                    }
                    Entry::Occupied(mut entry) => {
                        if plan_tie_key(&candidate) < plan_tie_key(entry.get()) {
                            entry.insert(candidate);
                        }
                    }
                }
            }
        }
        plans = next;
    }

    let best_meeting = plans
        .iter()
        .filter(|(values, _)| meets(**values, targets))
        .min_by_key(|(values, plan)| meeting_key(**values, targets, plan));
    if let Some((values, plan)) = best_meeting {
        return Ok(group_solution(sockets, *values, plan));
    }

    let best = plans
        .iter()
        .min_by_key(|(values, plan)| failure_key(**values, targets, plan))
        .map(|(values, plan)| group_solution(sockets, *values, plan));
    Err(GroupFailure { best, reason: None })
}

fn group_solution(sockets: &[usize], values: [u16; 3], plan: &Plan) -> GroupSolution {
    GroupSolution {
        assignments: sockets
            .iter()
            .copied()
            .zip(plan.hashes.iter().copied())
            .collect(),
        values,
    }
}

fn plan_tie_key(plan: &Plan) -> (usize, &[u64]) {
    (plan.changes, plan.hashes.as_slice())
}

fn meeting_key<const N: usize>(
    values: [u16; N],
    targets: [u16; N],
    plan: &Plan,
) -> (u32, Reverse<u32>, usize, &[u64]) {
    let excess = values
        .iter()
        .zip(targets)
        .filter(|(_, target)| *target > 0)
        .map(|(value, target)| u32::from(value.saturating_sub(target)))
        .sum();
    let total = values.iter().copied().map(u32::from).sum();
    (excess, Reverse(total), plan.changes, plan.hashes.as_slice())
}

fn failure_key<const N: usize>(
    values: [u16; N],
    targets: [u16; N],
    plan: &Plan,
) -> (u32, u16, Reverse<u32>, usize, &[u64]) {
    let shortfalls = values
        .iter()
        .zip(targets)
        .filter(|(_, target)| *target > 0)
        .map(|(value, target)| target.saturating_sub(*value))
        .collect::<Vec<_>>();
    let shortfall = shortfalls.iter().copied().map(u32::from).sum();
    let largest_shortfall = shortfalls.into_iter().max().unwrap_or_default();
    let total = values.iter().copied().map(u32::from).sum();
    (
        shortfall,
        largest_shortfall,
        Reverse(total),
        plan.changes,
        plan.hashes.as_slice(),
    )
}

pub(super) fn meets<const N: usize>(values: [u16; N], targets: [u16; N]) -> bool {
    values
        .iter()
        .zip(targets)
        .all(|(value, target)| target == 0 || *value >= target)
}

fn allocation_choices_for_socket(
    catalog: &Catalog,
    item: &ItemDef,
    socket_index: usize,
    current: Option<u64>,
    mode: PlugSelectionMode,
    group: AllocationGroup,
) -> Vec<Choice> {
    let hashes =
        crate::investment::plug_selection::candidates_for_socket(catalog, item, socket_index, mode);

    let mut by_values = HashMap::<[u16; 3], u64>::new();
    for hash in hashes {
        let Some((choice_group, values)) = parse_allocation_hash(catalog, hash) else {
            continue;
        };
        if choice_group != group {
            continue;
        }
        match by_values.entry(values) {
            Entry::Vacant(entry) => {
                entry.insert(hash);
            }
            Entry::Occupied(mut entry) => {
                if Some(hash) == current || (Some(*entry.get()) != current && hash < *entry.get()) {
                    entry.insert(hash);
                }
            }
        }
    }

    let mut choices = by_values
        .into_iter()
        .map(|(values, hash)| Choice { hash, values })
        .collect::<Vec<_>>();
    choices.sort_by_key(|choice| (Some(choice.hash) != current, choice.values, choice.hash));
    choices
}

fn cross_group_choices_for_socket(
    catalog: &Catalog,
    item: &ItemDef,
    socket_index: usize,
    current: Option<u64>,
    mode: PlugSelectionMode,
) -> Vec<CrossGroupChoice> {
    let mut hashes = match mode {
        PlugSelectionMode::GearType => catalog.gear_type_options(item, socket_index),
        PlugSelectionMode::AnyPlug => catalog.all_plug_options().to_vec(),
        PlugSelectionMode::Supported
        | PlugSelectionMode::SocketAndGearType
        | PlugSelectionMode::MatchingSocketType => Vec::new(),
    };
    if let Some(current) = current {
        hashes.push(current);
    }
    hashes.sort_unstable();
    hashes.dedup();

    let mut by_values = HashMap::<[u16; 6], u64>::new();
    for hash in hashes {
        let Some((group, values)) = parse_allocation_hash(catalog, hash) else {
            continue;
        };
        let values = expand_allocation_values(group, values);
        match by_values.entry(values) {
            Entry::Vacant(entry) => {
                entry.insert(hash);
            }
            Entry::Occupied(mut entry) => {
                if Some(hash) == current || (Some(*entry.get()) != current && hash < *entry.get()) {
                    entry.insert(hash);
                }
            }
        }
    }

    let mut choices = by_values
        .into_iter()
        .map(|(values, hash)| CrossGroupChoice { hash, values })
        .collect::<Vec<_>>();
    choices.sort_by_key(|choice| (Some(choice.hash) != current, choice.values, choice.hash));
    choices
}

pub(super) const fn expand_allocation_values(group: AllocationGroup, values: [u16; 3]) -> [u16; 6] {
    match group {
        AllocationGroup::Top => [values[0], values[1], values[2], 0, 0, 0],
        AllocationGroup::Bottom => [0, 0, 0, values[0], values[1], values[2]],
    }
}

pub(in crate::app::equipment) fn selected_totals(
    catalog: &Catalog,
    item: &ItemDef,
    plugs: &[Option<u64>],
) -> [u16; 6] {
    let mut totals = catalog.armor_stat_values(item.hash);
    let mut intrinsic_counted = false;
    for hash in plugs.iter().copied() {
        let Some(hash) = hash else {
            continue;
        };
        if is_intrinsic_plug(catalog, hash) {
            if intrinsic_counted {
                continue;
            }
            intrinsic_counted = true;
        }
        if let Some((plug_group, values)) = parse_allocation_hash(catalog, hash) {
            for (index, value) in plug_group.indices().into_iter().zip(values) {
                totals[index] = totals[index].saturating_add(i32::from(value));
            }
            continue;
        }
        let values = catalog.armor_stat_values(hash);
        for (total, value) in totals.iter_mut().zip(values) {
            *total = total.saturating_add(value);
        }
    }
    clamp_totals(totals)
}

/// Returns the stat contribution for a plug in one concrete socket. Allocation
/// definitions sometimes expose partial package rows, so their display-name
/// fallback is used independently of the destination socket's allocation group.
pub(in crate::app::equipment) fn socket_stat_values(
    catalog: &Catalog,
    _item: &ItemDef,
    _socket_index: usize,
    hash: u64,
) -> [i32; 6] {
    if let Some((group, values)) = parse_allocation_hash(catalog, hash) {
        expand_allocation_values(group, values).map(i32::from)
    } else {
        catalog.armor_stat_values(hash)
    }
}

fn fixed_non_allocation_values(
    catalog: &Catalog,
    item: &ItemDef,
    plugs: &[Option<u64>],
) -> [u16; 6] {
    let mut totals = catalog.armor_stat_values(item.hash);
    for (socket_index, hash) in plugs.iter().copied().enumerate() {
        if allocation_socket_group(catalog, item, socket_index).is_some() {
            continue;
        }
        let Some(hash) = hash else {
            continue;
        };
        for (total, value) in
            totals
                .iter_mut()
                .zip(socket_stat_values(catalog, item, socket_index, hash))
        {
            *total = total.saturating_add(value);
        }
    }
    clamp_totals(totals)
}

fn fixed_group_values(
    catalog: &Catalog,
    item: &ItemDef,
    plugs: &[Option<u64>],
    group: AllocationGroup,
) -> [u16; 3] {
    let mut totals = catalog.armor_stat_values(item.hash);
    for (socket_index, hash) in plugs.iter().copied().enumerate() {
        if allocation_socket_group(catalog, item, socket_index).is_some() {
            continue;
        }
        let Some(hash) = hash else {
            continue;
        };
        for (total, value) in
            totals
                .iter_mut()
                .zip(socket_stat_values(catalog, item, socket_index, hash))
        {
            *total = total.saturating_add(value);
        }
    }
    let totals = clamp_totals(totals);
    group.indices().map(|index| totals[index])
}

pub(super) fn remaining_targets(targets: [u16; 3], fixed: [u16; 3]) -> [u16; 3] {
    [
        targets[0].saturating_sub(fixed[0]),
        targets[1].saturating_sub(fixed[1]),
        targets[2].saturating_sub(fixed[2]),
    ]
}

pub(super) fn clamp_totals(totals: [i32; 6]) -> [u16; 6] {
    totals.map(|value| u16::try_from(value.max(0)).unwrap_or(u16::MAX))
}

pub(super) fn allocation_socket_groups(
    catalog: &Catalog,
    item: &ItemDef,
    plug_count: usize,
) -> [Option<AllocationGroup>; 2] {
    let mut groups = [None, None];
    for socket_index in 0..item.sockets.len().min(plug_count) {
        if let Some(group) = allocation_socket_group(catalog, item, socket_index) {
            groups[group_index(group)] = Some(group);
        }
    }
    groups
}

fn allocation_socket_indices(
    catalog: &Catalog,
    item: &ItemDef,
    plug_count: usize,
    group: AllocationGroup,
) -> Vec<usize> {
    (0..item.sockets.len().min(plug_count))
        .filter(|socket_index| allocation_socket_group(catalog, item, *socket_index) == Some(group))
        .collect()
}

fn allocation_socket_indices_all(
    catalog: &Catalog,
    item: &ItemDef,
    plug_count: usize,
) -> Vec<usize> {
    (0..item.sockets.len().min(plug_count))
        .filter(|socket_index| allocation_socket_group(catalog, item, *socket_index).is_some())
        .collect()
}

fn allocation_socket_group(
    catalog: &Catalog,
    item: &ItemDef,
    socket_index: usize,
) -> Option<AllocationGroup> {
    let socket = item.sockets.get(socket_index)?;
    match socket.socket_type {
        760 | 761 => return Some(AllocationGroup::Top),
        762 | 763 => return Some(AllocationGroup::Bottom),
        _ => {}
    }

    let label = socket.label.to_ascii_lowercase();
    if label.contains("top stat allocation") {
        return Some(AllocationGroup::Top);
    }
    if label.contains("bottom stat allocation") {
        return Some(AllocationGroup::Bottom);
    }

    item.default_plugs
        .get(socket_index)
        .and_then(Option::as_deref)
        .and_then(crate::hash::parse_hash_hex)
        .and_then(|hash| parse_allocation_hash(catalog, hash))
        .map(|(group, _)| group)
        .or_else(|| {
            catalog
                .socket_options(socket)
                .iter()
                .copied()
                .find_map(|hash| parse_allocation_hash(catalog, hash).map(|(group, _)| group))
        })
}

pub(in crate::app::equipment) fn is_allocation_socket(
    catalog: &Catalog,
    item: &ItemDef,
    socket_index: usize,
) -> bool {
    allocation_socket_group(catalog, item, socket_index).is_some()
}

pub(in crate::app::equipment) fn is_allocation_plug(catalog: &Catalog, hash: u64) -> bool {
    parse_allocation_hash(catalog, hash).is_some()
}

pub(in crate::app::equipment) fn is_intrinsic_plug(catalog: &Catalog, hash: u64) -> bool {
    catalog
        .plug_type_name(hash)
        .is_some_and(|name| name.to_ascii_lowercase().contains("intrinsic"))
}

fn parse_allocation_hash(catalog: &Catalog, hash: u64) -> Option<(AllocationGroup, [u16; 3])> {
    let (group, fallback) = parse_allocation_name(catalog.display_name(hash)?)?;
    let package = catalog.armor_stat_values(hash);
    let values = group.indices().map(|index| {
        u16::try_from(package[index])
            .ok()
            .filter(|value| *value > 0)
    });
    if let [Some(first), Some(second), Some(third)] = values {
        Some((group, [first, second, third]))
    } else {
        Some((group, fallback))
    }
}

pub(super) fn parse_allocation_name(name: &str) -> Option<(AllocationGroup, [u16; 3])> {
    let mut totals = [0_u16; 6];
    let mut seen = [false; 6];
    for segment in name.split('/') {
        let mut parts = segment.split_whitespace();
        let value = parts.next()?.parse::<u16>().ok()?;
        let stat = parts
            .next()?
            .trim_matches(|character: char| !character.is_ascii_alphabetic());
        let index = STAT_NAMES
            .iter()
            .position(|known| known.eq_ignore_ascii_case(stat))?;
        if seen[index] {
            return None;
        }
        totals[index] = value;
        seen[index] = true;
    }

    if seen[..3].iter().all(|seen| *seen) && seen[3..].iter().all(|seen| !*seen) {
        Some((AllocationGroup::Top, [totals[0], totals[1], totals[2]]))
    } else if seen[..3].iter().all(|seen| !*seen) && seen[3..].iter().all(|seen| *seen) {
        Some((AllocationGroup::Bottom, [totals[3], totals[4], totals[5]]))
    } else {
        None
    }
}

fn apply_assignments(plugs: &mut [Option<u64>], assignments: &[(usize, u64)]) {
    for (socket_index, hash) in assignments {
        plugs[*socket_index] = Some(*hash);
    }
}

pub(super) fn format_shortfalls(targets: [u16; 6], totals: [u16; 6]) -> String {
    STAT_NAMES
        .into_iter()
        .zip(targets.into_iter().zip(totals))
        .filter_map(|(name, (target, total))| {
            let shortfall = target.saturating_sub(total);
            (shortfall > 0).then(|| format!("{name} {shortfall}"))
        })
        .collect::<Vec<_>>()
        .join(" · ")
}
