use super::{
    model::{AllocationGroup, Choice, CrossGroupChoice},
    solver::{best_effort_failure, parse_allocation_name, solve_cross_group_plan, solve_group},
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
fn impossible_targets_return_the_closest_plug_plan_for_application() {
    let current = [Some(10), Some(20), Some(30)];
    let closest = vec![Some(11), Some(20), Some(31)];

    let failure = best_effort_failure(&current, closest.clone(), [30, 2, 2, 20, 4, 4], None);

    assert_eq!(failure.plugs, closest);
    assert_eq!(failure.changed, 2);
    assert_eq!(failure.best_totals, [30, 2, 2, 20, 4, 4]);
}
