use super::{model::CrossGroupChoice, solver::solve_cross_group_plan};

fn cross_choice(hash: u64, values: [u16; 6]) -> CrossGroupChoice {
    CrossGroupChoice { hash, values }
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
