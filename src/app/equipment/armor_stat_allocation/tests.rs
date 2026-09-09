use crate::app::PlugSelectionMode;

use super::{
    model::{AllocationGroup, Choice, CrossGroupChoice, mode_allows_cross_group_allocations},
    solver::{
        best_effort_failure, clamp_totals, expand_allocation_values, format_shortfalls, meets,
        parse_allocation_name, remaining_targets, solve_cross_group_plan, solve_group,
    },
};

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
        PlugSelectionMode::SocketAndGearType
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
        solve_cross_group_plan(&sockets, &choices, &current, [0; 6], [16, 0, 12, 0, 0, 0]).unwrap();

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
