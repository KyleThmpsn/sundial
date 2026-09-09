//! Armor-stat solver, preview, and application tests.

use super::*;
use crate::app::account_workspace::WorkspaceDocument;

#[test]
fn inventory_swap_choice_invalidates_the_armor_preview_source() {
    let document = WorkspaceDocument::json_only(serde_json::json!({}));
    let enabled = source_key(&document, 0, PlugSelectionMode::Supported, true);
    let disabled = source_key(&document, 0, PlugSelectionMode::Supported, false);
    assert_ne!(enabled, disabled);
}

#[test]
fn character_class_invalidates_the_armor_preview_source() {
    let mut document = WorkspaceDocument::json_only(serde_json::json!({
        "version": 8,
        "state": {"characters": [{"race": 0, "gender": 0, "class": 0}]}
    }));
    let titan = source_key(&document, 0, PlugSelectionMode::Supported, true);
    assert_eq!(titan.class_type, 0);

    document.json_mut()["state"]["characters"][0]["class"] = serde_json::json!(1);
    let hunter = source_key(&document, 0, PlugSelectionMode::Supported, true);
    assert_eq!(hunter.class_type, 1);
    assert_eq!(titan.equipment, hunter.equipment);
    assert_eq!(titan.inventory, hunter.inventory);
    assert_ne!(titan, hunter);
}

#[test]
fn preview_columns_fit_supported_window_widths() {
    for available in [520.0, 600.0, 760.0, 980.0] {
        let gap = 8.0;
        let widths = preview_column_widths(available, gap);
        let used = widths.into_iter().sum::<f32>() + 3.0 * gap;
        assert!(
            used <= available + f32::EPSILON,
            "preview used {used} px with {available} px available"
        );
    }
}

#[test]
fn reopening_resets_window_geometry_without_losing_targets() {
    let mut state = State::default();
    state.targets[1] = 100;

    state.open(0);
    let first_generation = state.window_generation;
    state.open = false;
    state.open(0);

    assert_eq!(state.targets[1], 100);
    assert_eq!(state.window_generation, first_generation.wrapping_add(1));
}

fn choice(hash: u64, values: [i32; 6]) -> SocketChoice {
    SocketChoice {
        hash: Some(hash),
        values,
    }
}

fn input(sockets: Vec<MutableSocket>, fixed_totals: [i32; 6]) -> LoadoutInput {
    let candidates = sockets
        .into_iter()
        .enumerate()
        .map(|(index, socket)| {
            vec![ArmorCandidate {
                piece: unavailable_piece("helmet", "Helmet", "test"),
                origin: ArmorOrigin::Equipped,
                fixed_totals: if index == 0 { fixed_totals } else { [0; 6] },
                sockets: vec![socket],
                exotic: false,
            }]
        })
        .collect::<Vec<_>>();
    LoadoutInput {
        pieces: (0..candidates.len())
            .map(|_| unavailable_piece("helmet", "Helmet", "test"))
            .collect(),
        candidates,
        current_totals: capped_totals(fixed_totals),
    }
}

fn socket(_index: usize, current: u64, choices: Vec<SocketChoice>) -> MutableSocket {
    MutableSocket {
        socket_index: 0,
        current: Some(current),
        choices,
        kind: SocketKind::Stat,
    }
}

#[test]
fn exact_goal_can_be_shared_across_multiple_armor_pieces() {
    let input = input(
        vec![
            socket(
                0,
                10,
                vec![choice(10, [0; 6]), choice(11, [0, 0, 10, 0, 0, 0])],
            ),
            socket(
                1,
                20,
                vec![choice(20, [0; 6]), choice(21, [0, 0, 20, 0, 0, 0])],
            ),
        ],
        [0, 0, 70, 0, 0, 0],
    );

    let solution = solve(&input, [0, 0, 100, 0, 0, 0]);

    assert!(solution.exact);
    assert_eq!(solution.projected_totals[2], 100);
    assert_eq!(solution.assignments.len(), 2);
    assert_eq!(changed_piece_count(&solution), 2);
}

#[test]
fn impossible_goal_returns_and_applies_the_closest_plan() {
    let input = input(
        vec![socket(
            0,
            10,
            vec![choice(10, [0; 6]), choice(11, [0, 0, 16, 0, 0, 0])],
        )],
        [0, 0, 80, 0, 0, 0],
    );

    let solution = solve(&input, [0, 0, 100, 0, 0, 0]);

    assert!(!solution.exact);
    assert_eq!(solution.projected_totals[2], 96);
    assert_eq!(solution.shortfalls[2], 4);
    assert_eq!(solution.assignments.len(), 1);
}

#[test]
fn zero_targets_are_ignored_and_current_choices_win_ties() {
    let input = input(
        vec![socket(
            0,
            10,
            vec![
                choice(10, [0, 0, 10, 0, 0, 0]),
                choice(11, [50, 0, 0, 0, 0, 0]),
            ],
        )],
        [50, 50, 90, 50, 50, 50],
    );

    let solution = solve(&input, [0, 0, 100, 0, 0, 0]);

    assert!(solution.exact);
    assert!(solution.assignments.is_empty());
}

#[test]
fn goals_are_minimums_and_already_met_stats_do_not_trigger_changes() {
    let input = input(
        vec![socket(
            0,
            10,
            vec![
                choice(10, [0, 0, 18, 0, 0, 0]),
                choice(11, [0, 0, 10, 0, 0, 0]),
            ],
        )],
        [0; 6],
    );

    let solution = solve(&input, [0, 0, 10, 0, 0, 0]);

    assert!(solution.exact);
    assert_eq!(solution.projected_totals[2], 18);
    assert!(solution.assignments.is_empty());
}

#[test]
fn multi_goal_solver_minimizes_total_shortfall_before_the_largest_gap() {
    let input = input(
        vec![socket(
            0,
            10,
            vec![
                choice(10, [0, 0, 0, 0, 0, 0]),
                choice(11, [20, 0, 2, 0, 0, 0]),
                choice(12, [10, 0, 10, 0, 0, 0]),
            ],
        )],
        [70, 0, 70, 0, 0, 0],
    );

    let solution = solve(&input, [90, 0, 90, 0, 0, 0]);

    assert_eq!(solution.projected_totals, [90, 0, 72, 0, 0, 0]);
    assert_eq!(solution.shortfalls, [0, 0, 18, 0, 0, 0]);
}

#[test]
fn useful_stat_range_is_clamped_to_the_real_cap() {
    assert_eq!(
        capped_totals([-1, 0, 50, 100, 101, i32::MAX]),
        [0, 0, 50, 100, 100, 100]
    );
    assert_eq!(
        cap_u16_totals([0, 50, 99, 100, 101, u16::MAX]),
        [0, 50, 99, 100, 100, 100]
    );
}

#[test]
fn solver_state_keys_cap_goals_and_compare_other_stats_separately() {
    assert_eq!(
        solver_key([10, 20, 130, 40, 150, 60], [0, 0, 100, 0, 100, 0]),
        [0, 0, 100, 0, 100, 0]
    );
}

#[test]
fn solver_removes_points_above_the_cap_when_an_exact_option_exists() {
    let input = input(
        vec![socket(
            0,
            10,
            vec![
                choice(10, [0, 0, 10, 0, 0, 0]),
                choice(11, [0, 0, 5, 0, 0, 0]),
            ],
        )],
        [0, 0, 95, 0, 0, 0],
    );

    let solution = solve(&input, [0, 0, 100, 0, 0, 0]);

    assert!(solution.exact);
    assert_eq!(solution.projected_totals[2], 100);
    assert_eq!(solution.assignments.len(), 1);
    assert_eq!(solution.assignments[0].selected, Some(11));
}

#[test]
fn candidate_reduction_keeps_differences_in_non_target_stats() {
    let socket = socket(
        0,
        10,
        vec![
            choice(10, [0, 0, 10, 0, 0, 0]),
            choice(11, [20, 0, 10, 0, 0, 0]),
        ],
    );

    assert_eq!(choices_for_targets(&socket, [0, 0, 100, 0, 0, 0]).len(), 2);
}

fn synthetic_candidate(
    fixed_totals: [i32; 6],
    origin: ArmorOrigin,
    sockets: Vec<MutableSocket>,
    exotic: bool,
) -> ArmorCandidate {
    ArmorCandidate {
        piece: unavailable_piece("helmet", "Helmet", "test"),
        origin,
        fixed_totals,
        sockets,
        exotic,
    }
}

#[test]
fn inventory_armor_is_selected_when_equipped_armor_cannot_reach_the_goal() {
    let input = LoadoutInput {
        pieces: vec![unavailable_piece("helmet", "Helmet", "test")],
        candidates: vec![vec![
            synthetic_candidate([0, 0, 80, 0, 0, 0], ArmorOrigin::Equipped, vec![], false),
            synthetic_candidate(
                [0, 0, 100, 0, 0, 0],
                ArmorOrigin::Inventory {
                    instance_soid: 42,
                    definition_hash: 7,
                },
                vec![],
                false,
            ),
        ]],
        current_totals: [0, 0, 80, 0, 0, 0],
    };

    let solution = solve(&input, [0, 0, 100, 0, 0, 0]);

    assert!(solution.exact);
    assert_eq!(solution.swaps.len(), 1);
    assert_eq!(solution.swaps[0].instance_soid, 42);
    assert_eq!(solution.selections[0].candidate_index, 1);
}

#[test]
fn masterworking_current_armor_is_preferred_to_an_inventory_swap() {
    let masterwork = MutableSocket {
        socket_index: 5,
        current: Some(10),
        choices: vec![choice(10, [0; 6]), choice(11, [0, 0, 12, 0, 0, 0])],
        kind: SocketKind::Masterwork,
    };
    let input = LoadoutInput {
        pieces: vec![unavailable_piece("helmet", "Helmet", "test")],
        candidates: vec![vec![
            synthetic_candidate(
                [0, 0, 88, 0, 0, 0],
                ArmorOrigin::Equipped,
                vec![masterwork],
                false,
            ),
            synthetic_candidate(
                [0, 0, 100, 0, 0, 0],
                ArmorOrigin::Inventory {
                    instance_soid: 42,
                    definition_hash: 7,
                },
                vec![],
                false,
            ),
        ]],
        current_totals: [0, 0, 88, 0, 0, 0],
    };

    let solution = solve(&input, [0, 0, 100, 0, 0, 0]);

    assert!(solution.exact);
    assert!(solution.swaps.is_empty());
    assert_eq!(solution.assignments.len(), 1);
    assert_eq!(solution.assignments[0].kind, SocketKind::Masterwork);
}

#[test]
fn optimizer_never_selects_two_exotic_armor_pieces() {
    let input = LoadoutInput {
        pieces: vec![
            unavailable_piece("helmet", "Helmet", "test"),
            unavailable_piece("gauntlets", "Gauntlets", "test"),
        ],
        candidates: vec![
            vec![
                synthetic_candidate([0; 6], ArmorOrigin::Equipped, vec![], false),
                synthetic_candidate(
                    [100, 0, 0, 0, 0, 0],
                    ArmorOrigin::Inventory {
                        instance_soid: 1,
                        definition_hash: 1,
                    },
                    vec![],
                    true,
                ),
            ],
            vec![
                synthetic_candidate([0; 6], ArmorOrigin::Equipped, vec![], false),
                synthetic_candidate(
                    [0, 100, 0, 0, 0, 0],
                    ArmorOrigin::Inventory {
                        instance_soid: 2,
                        definition_hash: 2,
                    },
                    vec![],
                    true,
                ),
            ],
        ],
        current_totals: [0; 6],
    };

    let solution = solve(&input, [100, 100, 0, 0, 0, 0]);

    assert!(!solution.exact);
    assert_eq!(solution.swaps.len(), 1);
    assert_eq!(
        solution
            .shortfalls
            .iter()
            .filter(|value| **value == 100)
            .count(),
        1
    );
}
