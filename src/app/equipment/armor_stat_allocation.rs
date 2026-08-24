//! Reusable armor-stat allocation targeting and presentation.
//!
//! Allocation plugs encode either the top stat group (Mobility, Resilience,
//! Recovery) or the bottom stat group (Discipline, Intellect, Strength). This
//! module keeps target entry, compatible-pool solving, and result feedback
//! independent from the random-item workspace so other item editors can reuse it.

use std::{
    cmp::Reverse,
    collections::{HashMap, hash_map::Entry},
};

use eframe::egui;

use crate::{
    app::PlugSelectionMode,
    catalog::{Catalog, ItemDef},
};

pub(super) const STAT_NAMES: [&str; 6] = [
    "Mobility",
    "Resilience",
    "Recovery",
    "Discipline",
    "Intellect",
    "Strength",
];
pub(super) const TARGET_MAX: u16 = 100;
pub(super) const INLINE_CONTENT_WIDTH: f32 = 582.0;
const STAT_CELL_WIDTH: f32 = 150.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AllocationGroup {
    Top,
    Bottom,
}

impl AllocationGroup {
    const ALL: [Self; 2] = [Self::Top, Self::Bottom];

    const fn indices(self) -> [usize; 3] {
        match self {
            Self::Top => [0, 1, 2],
            Self::Bottom => [3, 4, 5],
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Top => "Top allocation",
            Self::Bottom => "Bottom allocation",
        }
    }
}

#[derive(Clone, Debug)]
struct AllocationFeedback {
    text: String,
    is_error: bool,
}

#[derive(Clone, Debug, Default)]
pub(super) struct State {
    targets: [u16; 6],
    feedback: Option<AllocationFeedback>,
}

impl State {
    pub(super) fn clear_feedback(&mut self) {
        self.feedback = None;
    }

    pub(super) fn feedback(&self) -> Option<(&str, bool)> {
        self.feedback
            .as_ref()
            .map(|feedback| (feedback.text.as_str(), feedback.is_error))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Choice {
    hash: u64,
    values: [u16; 3],
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CrossGroupChoice {
    hash: u64,
    values: [u16; 6],
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Plan {
    hashes: Vec<u64>,
    changes: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct GroupSolution {
    assignments: Vec<(usize, u64)>,
    values: [u16; 3],
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct GroupFailure {
    best: Option<GroupSolution>,
    reason: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CrossGroupSolution {
    assignments: Vec<(usize, u64)>,
    values: [u16; 6],
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CrossGroupFailure {
    best: Option<CrossGroupSolution>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Solution {
    plugs: Vec<Option<u64>>,
    changed: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SolveFailure {
    plugs: Vec<Option<u64>>,
    changed: usize,
    best_totals: [u16; 6],
    reason: Option<String>,
}

/// Draws the compact target editor and applies a solved allocation directly to
/// `plugs` when requested. Returns true only when the authored plug list changed.
pub(super) fn draw(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    item: &ItemDef,
    plugs: &mut [Option<u64>],
    mode: PlugSelectionMode,
    state: &mut State,
) -> bool {
    let socket_groups = allocation_socket_groups(catalog, item, plugs.len());
    if socket_groups.iter().all(Option::is_none) {
        return false;
    }

    let current = selected_totals(catalog, item, plugs);
    let mut solve_requested = false;
    let mut clear_requested = false;
    let mut changed = false;

    ui.horizontal_top(|ui| {
        ui.add_space((ui.available_width() - INLINE_CONTENT_WIDTH).max(0.0));
        ui.spacing_mut().item_spacing.x = 5.0;
        let cross_group = mode_allows_cross_group_allocations(mode);
        let any_allocation_socket = socket_groups.iter().any(Option::is_some);
        let targets_changed = draw_stat_cells(
            ui,
            catalog,
            &mut state.targets,
            current,
            if cross_group {
                [any_allocation_socket; 6]
            } else {
                [
                    socket_groups[0].is_some(),
                    socket_groups[0].is_some(),
                    socket_groups[0].is_some(),
                    socket_groups[1].is_some(),
                    socket_groups[1].is_some(),
                    socket_groups[1].is_some(),
                ]
            },
        );
        if targets_changed {
            state.feedback = None;
        }

        ui.with_layout(egui::Layout::top_down(egui::Align::RIGHT), |ui| {
            let has_targets = state.targets.iter().any(|target| *target > 0);
            if ui
                .add_enabled(has_targets, egui::Button::new("Adjust stats"))
                .on_disabled_hover_text("Set at least one target first")
                .clicked()
            {
                solve_requested = true;
            }
            if ui
                .add_enabled(has_targets, egui::Button::new("Clear"))
                .clicked()
            {
                clear_requested = true;
            }
        });
    });

    if clear_requested {
        state.targets = [0; 6];
        state.feedback = None;
    } else if solve_requested {
        match solve(catalog, item, plugs, mode, state.targets) {
            Ok(solution) => {
                changed = solution.changed > 0;
                plugs.copy_from_slice(&solution.plugs);
                state.feedback = Some(AllocationFeedback {
                    text: if solution.changed == 0 {
                        "Targets already met".to_owned()
                    } else {
                        "Stats adjusted · Targets met".to_owned()
                    },
                    is_error: false,
                });
            }
            Err(failure) => {
                let shortfalls = format_shortfalls(state.targets, failure.best_totals);
                changed = failure.changed > 0;
                plugs.copy_from_slice(&failure.plugs);
                state.feedback = Some(AllocationFeedback {
                    text: if shortfalls.is_empty() {
                        failure
                            .reason
                            .unwrap_or_else(|| "No closer stat combination is available".to_owned())
                    } else {
                        format!("Closest available · {shortfalls} short")
                    },
                    is_error: true,
                });
            }
        }
    }

    changed
}

pub(super) fn is_available(catalog: &Catalog, item: &ItemDef, plug_count: usize) -> bool {
    allocation_socket_groups(catalog, item, plug_count)
        .iter()
        .any(Option::is_some)
}

fn draw_stat_cells(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    targets: &mut [u16; 6],
    current: [u16; 6],
    available: [bool; 6],
) -> bool {
    let mut changed = false;
    egui::Grid::new(ui.id().with("armor-stat-target-grid"))
        .num_columns(3)
        .spacing(egui::vec2(4.0, 3.0))
        .show(ui, |ui| {
            for row in 0..2 {
                for index in (row * 3)..(row * 3 + 3) {
                    changed |= draw_stat_cell(ui, catalog, targets, current, available, index);
                }
                ui.end_row();
            }
        });
    changed
}

fn draw_stat_cell(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    targets: &mut [u16; 6],
    current: [u16; 6],
    available: [bool; 6],
    index: usize,
) -> bool {
    ui.allocate_ui_with_layout(
        egui::vec2(STAT_CELL_WIDTH, 21.0),
        egui::Layout::right_to_left(egui::Align::Center),
        |ui| {
            ui.set_min_width(STAT_CELL_WIDTH);
            ui.spacing_mut().item_spacing.x = 3.0;
            let target_ui = ui.add_enabled_ui(available[index], |ui| {
                ui.add_sized(
                    [34.0, ui.spacing().interact_size.y],
                    egui::DragValue::new(&mut targets[index])
                        .range(0..=TARGET_MAX)
                        .speed(1.0),
                )
            });
            let response = target_ui.inner;
            response.widget_info(|| {
                let mut info =
                    egui::WidgetInfo::drag_value(available[index], f64::from(targets[index]));
                info.label = Some(format!("{} target", STAT_NAMES[index]));
                info
            });
            let target_changed = response.changed();
            let tooltip = format!(
                "Minimum {} to reach with Adjust stats. Set to 0 to ignore this stat.",
                STAT_NAMES[index]
            );
            response
                .on_disabled_hover_text("This stat has no allocation socket on the selected item")
                .on_hover_text(&tooltip);
            target_ui
                .response
                .on_disabled_hover_text("This stat has no allocation socket on the selected item")
                .on_hover_text(tooltip);
            let unmet = targets[index] > 0 && current[index] < targets[index];
            let text = egui::RichText::new(current[index].to_string()).small();
            if unmet {
                ui.add_sized(
                    [22.0, ui.spacing().interact_size.y],
                    egui::Label::new(text.color(ui.visuals().error_fg_color)),
                )
                .on_hover_text("Current item stat");
            } else {
                ui.add_sized([22.0, ui.spacing().interact_size.y], egui::Label::new(text))
                    .on_hover_text("Current item stat");
            }
            ui.add_enabled_ui(available[index], |ui| {
                ui.horizontal(|ui| {
                    if let Some(icon) = catalog.armor_stat_icon_texture(ui.ctx(), STAT_NAMES[index])
                    {
                        ui.add(egui::Image::new((icon.id(), egui::vec2(14.0, 14.0))))
                            .on_hover_text(STAT_NAMES[index]);
                    }
                    ui.label(STAT_NAMES[index]);
                });
            });
            target_changed
        },
    )
    .inner
}

fn solve(
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

fn solve_cross_group_plan(
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
        .filter(|(values, _)| meets_six(add_stat_values(fixed, **values), targets))
        .min_by_key(|(values, plan)| {
            meeting_key_six(add_stat_values(fixed, **values), targets, plan)
        });
    if let Some((values, plan)) = best_meeting {
        return Ok(cross_group_solution(
            sockets,
            add_stat_values(fixed, *values),
            plan,
        ));
    }

    let best = plans
        .iter()
        .min_by_key(|(values, plan)| {
            failure_key_six(add_stat_values(fixed, **values), targets, plan)
        })
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

fn meets_six(values: [u16; 6], targets: [u16; 6]) -> bool {
    values
        .iter()
        .zip(targets)
        .all(|(value, target)| target == 0 || *value >= target)
}

fn meeting_key_six(
    values: [u16; 6],
    targets: [u16; 6],
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

fn failure_key_six(
    values: [u16; 6],
    targets: [u16; 6],
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

fn best_effort_failure(
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

fn solve_group(
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

fn meeting_key(
    values: [u16; 3],
    targets: [u16; 3],
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

fn failure_key(
    values: [u16; 3],
    targets: [u16; 3],
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

fn meets(values: [u16; 3], targets: [u16; 3]) -> bool {
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
    let Some(socket) = item.sockets.get(socket_index) else {
        return Vec::new();
    };
    let hashes = match mode {
        PlugSelectionMode::Supported => catalog.socket_options(socket).to_vec(),
        PlugSelectionMode::MatchingSocketType => {
            catalog.socket_type_options(socket.socket_type).to_vec()
        }
        PlugSelectionMode::GearType => catalog.gear_type_options(item, socket_index),
        PlugSelectionMode::AnyPlug => catalog.all_plug_options().to_vec(),
    };

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
        PlugSelectionMode::Supported | PlugSelectionMode::MatchingSocketType => Vec::new(),
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

const fn expand_allocation_values(group: AllocationGroup, values: [u16; 3]) -> [u16; 6] {
    match group {
        AllocationGroup::Top => [values[0], values[1], values[2], 0, 0, 0],
        AllocationGroup::Bottom => [0, 0, 0, values[0], values[1], values[2]],
    }
}

pub(super) fn selected_totals(
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
pub(super) fn socket_stat_values(
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

fn remaining_targets(targets: [u16; 3], fixed: [u16; 3]) -> [u16; 3] {
    [
        targets[0].saturating_sub(fixed[0]),
        targets[1].saturating_sub(fixed[1]),
        targets[2].saturating_sub(fixed[2]),
    ]
}

fn clamp_totals(totals: [i32; 6]) -> [u16; 6] {
    totals.map(|value| u16::try_from(value.max(0)).unwrap_or(u16::MAX))
}

fn allocation_socket_groups(
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

pub(super) fn is_allocation_socket(catalog: &Catalog, item: &ItemDef, socket_index: usize) -> bool {
    allocation_socket_group(catalog, item, socket_index).is_some()
}

pub(super) fn is_allocation_plug(catalog: &Catalog, hash: u64) -> bool {
    parse_allocation_hash(catalog, hash).is_some()
}

pub(super) fn is_intrinsic_plug(catalog: &Catalog, hash: u64) -> bool {
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

fn parse_allocation_name(name: &str) -> Option<(AllocationGroup, [u16; 3])> {
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

fn format_shortfalls(targets: [u16; 6], totals: [u16; 6]) -> String {
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

const fn group_index(group: AllocationGroup) -> usize {
    match group {
        AllocationGroup::Top => 0,
        AllocationGroup::Bottom => 1,
    }
}

const fn mode_allows_cross_group_allocations(mode: PlugSelectionMode) -> bool {
    matches!(
        mode,
        PlugSelectionMode::GearType | PlugSelectionMode::AnyPlug
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn choice(hash: u64, values: [u16; 3]) -> Choice {
        Choice { hash, values }
    }

    fn cross_choice(hash: u64, values: [u16; 6]) -> CrossGroupChoice {
        CrossGroupChoice { hash, values }
    }

    #[test]
    fn parses_top_and_bottom_allocation_names() {
        assert_eq!(
            parse_allocation_name("8 Mobility / 1 Resilience / 6 Recovery"),
            Some((AllocationGroup::Top, [8, 1, 6]))
        );
        assert_eq!(
            parse_allocation_name("1 Discipline / 9 Intellect / 5 Strength"),
            Some((AllocationGroup::Bottom, [1, 9, 5]))
        );
        assert_eq!(parse_allocation_name("Mobility Mod"), None);
    }

    #[test]
    fn allocation_values_follow_the_plug_group_not_the_socket_group() {
        assert_eq!(
            expand_allocation_values(AllocationGroup::Top, [8, 1, 6]),
            [8, 1, 6, 0, 0, 0]
        );
        assert_eq!(
            expand_allocation_values(AllocationGroup::Bottom, [1, 9, 5]),
            [0, 0, 0, 1, 9, 5]
        );
    }

    #[test]
    fn only_gear_type_and_all_allow_cross_group_allocations() {
        assert!(!mode_allows_cross_group_allocations(
            PlugSelectionMode::Supported
        ));
        assert!(!mode_allows_cross_group_allocations(
            PlugSelectionMode::MatchingSocketType
        ));
        assert!(mode_allows_cross_group_allocations(
            PlugSelectionMode::GearType
        ));
        assert!(mode_allows_cross_group_allocations(
            PlugSelectionMode::AnyPlug
        ));
    }

    #[test]
    fn cross_group_solver_can_fill_nominal_bottom_sockets_with_top_plugs() {
        // The solver intentionally receives no destination-group metadata. In
        // Gear type and All modes, either allocation family may fill either
        // allocation socket and the chosen plug keeps its real stat values.
        let sockets = [2, 3];
        let choices = vec![
            vec![
                cross_choice(10, [8, 1, 6, 0, 0, 0]),
                cross_choice(11, [0, 0, 0, 1, 9, 5]),
            ],
            vec![
                cross_choice(20, [8, 1, 6, 0, 0, 0]),
                cross_choice(21, [0, 0, 0, 1, 9, 5]),
            ],
        ];
        let current = [None, None, Some(11), Some(21)];

        let solution =
            solve_cross_group_plan(&sockets, &choices, &current, [0; 6], [16, 0, 12, 0, 0, 0])
                .unwrap();

        assert_eq!(solution.values, [16, 2, 12, 0, 0, 0]);
        assert_eq!(solution.assignments, vec![(2, 10), (3, 20)]);
    }

    #[test]
    fn solver_selects_the_smallest_compatible_excess() {
        let sockets = [2, 3];
        let choices = vec![
            vec![choice(10, [8, 1, 6]), choice(11, [5, 5, 1])],
            vec![choice(20, [8, 1, 6]), choice(21, [5, 5, 1])],
        ];
        let current = [None, None, Some(10), Some(20)];

        let solution = solve_group(&sockets, &choices, &current, [10, 6, 0]).unwrap();

        assert_eq!(solution.values, [13, 6, 7]);
        assert_eq!(solution.assignments, vec![(2, 10), (3, 21)]);
    }

    #[test]
    fn solver_keeps_the_current_hash_when_equal_values_tie() {
        let sockets = [0];
        let choices = vec![vec![choice(10, [8, 1, 6]), choice(11, [8, 1, 6])]];
        let current = [Some(11)];

        let solution = solve_group(&sockets, &choices, &current, [8, 1, 6]).unwrap();

        assert_eq!(solution.assignments, vec![(0, 11)]);
    }

    #[test]
    fn solver_reports_the_closest_available_shortfall() {
        let sockets = [0];
        let choices = vec![vec![choice(10, [8, 1, 6]), choice(11, [5, 5, 1])]];
        let current = [Some(10)];

        let failure = solve_group(&sockets, &choices, &current, [10, 5, 0]).unwrap_err();

        assert_eq!(failure.best.unwrap().values, [5, 5, 1]);
    }

    #[test]
    fn zero_targets_are_ignored_when_testing_requirements() {
        assert!(meets([1, 10, 1], [0, 10, 0]));
        assert!(!meets([20, 9, 20], [0, 10, 0]));
    }

    #[test]
    fn failure_copy_only_names_stats_that_miss_their_targets() {
        assert_eq!(
            format_shortfalls([30, 10, 0, 10, 0, 0], [22, 12, 4, 7, 20, 8]),
            "Mobility 8 · Discipline 3"
        );
    }

    #[test]
    fn fixed_item_and_mod_stats_reduce_the_required_allocation() {
        assert_eq!(remaining_targets([30, 10, 0], [12, 10, 40]), [18, 0, 0]);
    }

    #[test]
    fn signed_package_totals_are_clamped_for_display() {
        assert_eq!(
            clamp_totals([-10, 12, i32::MAX, 0, 7, 3]),
            [0, 12, u16::MAX, 0, 7, 3]
        );
    }

    #[test]
    fn impossible_targets_return_the_closest_plug_plan_for_application() {
        let current = [Some(10), Some(20), Some(30)];
        let closest = vec![Some(11), Some(20), Some(31)];

        let failure = best_effort_failure(&current, closest.clone(), [30, 2, 2, 20, 4, 4], None);

        assert_eq!(failure.plugs, closest);
        assert_eq!(failure.changed, 2);
        assert_eq!(failure.best_totals, [30, 2, 2, 20, 4, 4]);
    }
}
