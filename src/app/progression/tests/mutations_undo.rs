use super::*;

#[test]
fn flag_mutations_split_and_rejoin_runs_without_touching_unknown_fields() {
    let mut document = json!({
        "state": {
            "unlocks": {
                "account_flag_runs": [[1, 3]],
                "future_field": {"preserved": true}
            }
        }
    });

    assert!(set_unlock_flag(
        &mut document,
        "account_flag_runs",
        2,
        false
    ));
    assert_eq!(
        document.pointer("/state/unlocks/account_flag_runs"),
        Some(&json!([[1, 1], [3, 1]]))
    );
    assert!(set_unlock_flag(&mut document, "account_flag_runs", 2, true));
    assert_eq!(
        document.pointer("/state/unlocks/account_flag_runs"),
        Some(&json!([[1, 3]]))
    );
    assert_eq!(
        document.pointer("/state/unlocks/future_field/preserved"),
        Some(&json!(true))
    );
}

#[test]
fn indexed_flag_and_value_mutations_are_sorted_and_removable() {
    let mut document = json!({
        "state": {"unlocks": {"character_flags": [9, 3], "objective_values": [[8, 1]]}}
    });

    assert!(set_unlock_flag(&mut document, "character_flags", 5, true));
    assert_eq!(
        document.pointer("/state/unlocks/character_flags"),
        Some(&json!([3, 5, 9]))
    );
    assert!(set_unlock_value(&mut document, "objective_values", 4, -7));
    assert!(set_unlock_value(&mut document, "objective_values", 8, 12));
    assert_eq!(
        document.pointer("/state/unlocks/objective_values"),
        Some(&json!([[4, -7], [8, 12]]))
    );
    assert!(remove_unlock_value(&mut document, "objective_values", 4));
    assert_eq!(
        document.pointer("/state/unlocks/objective_values"),
        Some(&json!([[8, 12]]))
    );
}

#[test]
fn progression_mutations_are_sorted_updated_and_removable() {
    let mut document = json!({
        "future_root": {"preserved": true},
        "state": {"unlocks": {"future_unlock": ["preserved"]}}
    });

    assert!(set_progression_value(
        &mut document,
        "account_progressions",
        5,
        [1, 2, 3],
    ));
    assert!(set_progression_value(
        &mut document,
        "account_progressions",
        2,
        [-1, 0, 9],
    ));
    assert!(set_progression_value(
        &mut document,
        "account_progressions",
        5,
        [4, 5, 6],
    ));
    assert_eq!(
        document.pointer("/state/unlocks/account_progressions"),
        Some(&json!([[2, -1, 0, 9], [5, 4, 5, 6]]))
    );
    assert!(remove_progression_value(
        &mut document,
        "account_progressions",
        2,
    ));
    assert_eq!(
        document.pointer("/state/unlocks/account_progressions"),
        Some(&json!([[5, 4, 5, 6]]))
    );
    assert_eq!(
        document.pointer("/state/unlocks/future_unlock"),
        Some(&json!(["preserved"]))
    );
    assert_eq!(
        document.pointer("/future_root/preserved"),
        Some(&json!(true))
    );
}

#[test]
fn progression_undo_restores_rows_and_clears_matching_dirty_state() {
    let mut document = json!({
        "state": {"unlocks": {
            "account_progressions": [[23, 1, 2, 3]]
        }}
    });
    let mut state = UiState::default();

    assert!(set_progression_value(
        &mut document,
        "account_progressions",
        23,
        [9, 8, 7],
    ));
    state.record_progression_change("account_progressions", 23, Some([1, 2, 3]), Some([9, 8, 7]));
    assert!(state.progression_changed("account_progressions", 23));
    assert!(undo_progression_change(&mut document, &mut state));
    assert_eq!(
        saved_progression_lanes(&document, ProgressionScope::Account, 23),
        Some([1, 2, 3])
    );
    assert!(!state.progression_changed("account_progressions", 23));

    assert!(set_progression_value(
        &mut document,
        "account_progressions",
        24,
        [0; 3],
    ));
    state.record_progression_change("account_progressions", 24, None, Some([0; 3]));
    assert!(undo_progression_change(&mut document, &mut state));
    assert_eq!(
        saved_progression_lanes(&document, ProgressionScope::Account, 24),
        None
    );
    assert!(!state.progression_changed("account_progressions", 24));
}

#[test]
fn family5_overrides_add_edit_and_remove_the_selected_definition() {
    let mut document = json!({"state": {"investment": {}}});

    assert!(set_investment_override(
        &mut document,
        InvestmentTable::FlagOverrides,
        2003,
        2
    ));
    assert!(set_investment_override(
        &mut document,
        InvestmentTable::ValueOverrides,
        3510,
        -5
    ));
    assert_eq!(
        document.pointer("/state/investment/family5_flag_overrides"),
        Some(&json!([[2003, 2]]))
    );
    assert_eq!(
        document.pointer("/state/investment/family5_value_overrides"),
        Some(&json!([[3510, -5]]))
    );
    assert!(remove_investment_override(
        &mut document,
        InvestmentTable::FlagOverrides,
        2003
    ));
    assert_eq!(
        document.pointer("/state/investment/family5_flag_overrides"),
        Some(&json!([]))
    );
}

#[test]
fn investment_undo_restores_edits_and_removes_new_rows() {
    let mut document = json!({
        "state": {"investment": {
            "family5_flag_overrides": [[2003, 2]],
            "family5_value_overrides": [[3510, 9]]
        }}
    });
    let mut state = UiState {
        last_investment_change: Some(InvestmentUndo::Flag {
            definition_index: 2003,
            previous: None,
        }),
        ..UiState::default()
    };
    assert!(undo_investment_change(&mut document, &mut state));
    assert_eq!(
        document.pointer("/state/investment/family5_flag_overrides"),
        Some(&json!([]))
    );
    assert!(state.last_investment_change.is_none());

    state.last_investment_change = Some(InvestmentUndo::Value {
        definition_index: 3510,
        previous: Some(7),
    });
    assert!(undo_investment_change(&mut document, &mut state));
    assert_eq!(
        document.pointer("/state/investment/family5_value_overrides"),
        Some(&json!([[3510, 7]]))
    );
}

#[test]
fn mutations_update_the_effective_last_duplicate() {
    let mut document = json!({
        "state": {
            "investment": {
                "family5_flag_overrides": [[2003, 1], [2003, 2]],
                "family5_value_overrides": [[3510, 4], [3510, 9]]
            },
            "unlocks": {
                "objective_values": [[8, 1], [8, 2]],
                "account_progressions": [[23, 1, 2, 3], [23, 4, 5, 6]]
            }
        }
    });

    assert!(set_investment_override(
        &mut document,
        InvestmentTable::FlagOverrides,
        2003,
        0
    ));
    assert!(set_investment_override(
        &mut document,
        InvestmentTable::ValueOverrides,
        3510,
        10
    ));
    assert!(set_unlock_value(&mut document, "objective_values", 8, 7));
    assert!(set_progression_value(
        &mut document,
        "account_progressions",
        23,
        [7, 8, 9]
    ));

    assert_eq!(
        document.pointer("/state/investment/family5_flag_overrides"),
        Some(&json!([[2003, 1], [2003, 0]]))
    );
    assert_eq!(
        document.pointer("/state/investment/family5_value_overrides"),
        Some(&json!([[3510, 4], [3510, 10]]))
    );
    assert_eq!(
        document.pointer("/state/unlocks/objective_values"),
        Some(&json!([[8, 1], [8, 7]]))
    );
    assert_eq!(
        document.pointer("/state/unlocks/account_progressions"),
        Some(&json!([[23, 7, 8, 9]]))
    );
}

#[test]
fn reserved_character_objective_values_cannot_be_changed_or_removed() {
    let mut document = json!({
        "state": {"unlocks": {"character_objective_values": [[443, -1], [502, -1]]}}
    });

    assert!(!set_unlock_value(
        &mut document,
        "character_object_objective_values",
        443,
        0
    ));
    assert!(!remove_unlock_value(
        &mut document,
        "character_object_objective_values",
        502
    ));
    assert_eq!(
        document.pointer("/state/unlocks/character_objective_values"),
        Some(&json!([[443, -1], [502, -1]]))
    );

    let invalid = json!({
        "state": {"unlocks": {"character_objective_values": [[443, 0]]}}
    });
    assert!(
        parse(&invalid)
            .unwrap_err()
            .contains("slot 443 must remain -1")
    );
}

#[test]
fn collection_mutations_update_an_existing_family5_override_instead_of_hidden_compact_state() {
    let mut document = json!({
        "state": {
            "unlocks": {
                "account_flag_runs": [[4, 1]],
                "objective_values": [[5, 7]]
            },
            "investment": {
                "family5_flag_overrides": [[10, 0]],
                "family5_value_overrides": [[11, 99]]
            }
        }
    });
    let flag = UnlockDefinition {
        code: ACCOUNT_FLAG_BANK.into(),
        compact_slot: Some(4),
        ..UnlockDefinition::default()
    };
    let value = UnlockDefinition {
        code: ACCOUNT_OBJECTIVE_BANK.into(),
        compact_slot: Some(5),
        ..UnlockDefinition::default()
    };

    assert!(set_collection_flag(&mut document, 10, &flag, true));
    assert!(set_collection_value(&mut document, 11, &value, 123));
    assert_eq!(
        document.pointer("/state/investment/family5_flag_overrides"),
        Some(&json!([[10, 2]]))
    );
    assert_eq!(
        document.pointer("/state/investment/family5_value_overrides"),
        Some(&json!([[11, 123]]))
    );
    assert_eq!(
        document.pointer("/state/unlocks/account_flag_runs"),
        Some(&json!([[4, 1]]))
    );
    assert_eq!(
        document.pointer("/state/unlocks/objective_values"),
        Some(&json!([[5, 7]]))
    );

    let snapshot = collection_state_snapshot(&document).unwrap();
    assert_eq!(snapshot.flag_value(10, &flag), Some(true));
    assert_eq!(snapshot.value(11, &value), Some(123));
}
