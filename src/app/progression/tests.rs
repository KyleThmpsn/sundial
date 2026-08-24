use super::*;
use super::{hierarchy::*, mutations::*, override_tables::*, state::*, unlock_tables::*};

use serde_json::json;

#[test]
fn progression_tables_include_package_definitions_without_authored_values() {
    let definitions = [
        ProgressionDefinition {
            definition_index: 3,
            hash: 0x1111_1111,
            scope: ProgressionScope::Account,
            scope_slot: Some(0),
            repeat_last_step: false,
            name: String::new(),
            description: String::new(),
            source: String::new(),
            display_units_name: String::new(),
            icon_container: None,
            factions: Vec::new(),
            steps: Vec::new(),
            reward_items: Vec::new(),
        },
        ProgressionDefinition {
            definition_index: 7,
            hash: 0x2222_2222,
            scope: ProgressionScope::Account,
            scope_slot: Some(1),
            repeat_last_step: false,
            name: String::new(),
            description: String::new(),
            source: String::new(),
            display_units_name: String::new(),
            icon_container: None,
            factions: Vec::new(),
            steps: Vec::new(),
            reward_items: Vec::new(),
        },
        ProgressionDefinition {
            definition_index: 9,
            hash: 0x3333_3333,
            scope: ProgressionScope::Character,
            scope_slot: Some(0),
            repeat_last_step: false,
            name: String::new(),
            description: String::new(),
            source: String::new(),
            display_units_name: String::new(),
            icon_container: None,
            factions: Vec::new(),
            steps: Vec::new(),
            reward_items: Vec::new(),
        },
    ];
    let authored = [ProgressionValue {
        definition_index: 7,
        lanes: [1, 2, 3],
    }];

    assert_eq!(
        progression_display_rows(&authored, &definitions, ProgressionScope::Account),
        [
            ProgressionDisplayRow {
                definition_index: 3,
                lanes: None,
            },
            ProgressionDisplayRow {
                definition_index: 7,
                lanes: Some([1, 2, 3]),
            },
        ]
    );
}

#[test]
fn progression_definition_search_includes_package_step_metadata() {
    let definition: ProgressionDefinition = serde_json::from_value(serde_json::json!({
        "definition_index": 4,
        "hash": 0x1234_ABCD_u64,
        "scope": "Unreplicated",
        "scope_slot": null,
        "repeat_last_step": false,
        "name": "Crucible Rank",
        "description": "Earn rank progress",
        "source": "Complete matches",
        "steps": [{ "progress_total": 850, "name": "Heroic" }]
    }))
    .unwrap();

    assert!(progression_definition_matches("crucible", &definition));
    assert!(progression_definition_matches("complete", &definition));
    assert!(progression_definition_matches("heroic", &definition));
    assert!(progression_definition_matches("850", &definition));
    assert!(progression_definition_matches("1234abcd", &definition));
    assert!(!progression_definition_matches("vanguard", &definition));
}

#[test]
fn progression_save_state_distinguishes_missing_rows_and_merges_duplicates() {
    let document = json!({
        "state": {"unlocks": {
            "account_progressions": [
                [23, 5, 1, -2],
                [23, 8, 0, 4]
            ]
        }}
    });

    assert_eq!(
        saved_progression_lanes(&document, ProgressionScope::Account, 23),
        Some([8, 1, 4])
    );
    assert_eq!(
        saved_progression_lanes(&document, ProgressionScope::Account, 24),
        None
    );
    assert_eq!(
        saved_progression_lanes(&document, ProgressionScope::Character, 23),
        None
    );
}

#[test]
fn progression_target_uses_the_highest_package_progress_total() {
    let definition: ProgressionDefinition = serde_json::from_value(json!({
        "definition_index": 4,
        "hash": 1,
        "scope": "Account",
        "scope_slot": 0,
        "repeat_last_step": false,
        "steps": [
            {"progress_total": 100},
            {"progress_total": 50},
            {"progress_total": 250}
        ]
    }))
    .unwrap();

    assert_eq!(progression_target(&definition), Some(250));
}

#[test]
fn cached_optional_sort_builds_each_key_once_and_keeps_missing_values_last() {
    let mut rows = vec![Some("bravo"), None, Some("alpha")];
    let mut calls = 0;

    sort_by_optional_cached_key(&mut rows, false, |row| {
        calls += 1;
        row.map(str::to_owned)
    });

    assert_eq!(calls, rows.len());
    assert_eq!(rows, [Some("alpha"), Some("bravo"), None]);

    sort_by_optional_cached_key(&mut rows, true, |row| row.map(str::to_owned));
    assert_eq!(rows, [Some("bravo"), Some("alpha"), None]);
}

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
fn override_coverage_filters_distinguish_mapping_and_decode_confidence() {
    let unresolved = UnlockDefinition {
        hash: 1,
        code: 0,
        compact_slot: None,
        name: None,
        description: None,
        tested_by: Vec::new(),
    };
    let partial = UnlockDefinition {
        tested_by: vec![ProgressionContextDef {
            hash: 2,
            kind: ProgressionContextKind::Activity,
            name: String::new(),
            type_name: String::new(),
            description: String::new(),
            paths: Vec::new(),
            condition_programs: vec![vec![[99, 1]]],
        }],
        ..unresolved.clone()
    };

    assert!(override_filter_matches(OverrideFilter::Unmapped, None));
    assert!(override_filter_matches(
        OverrideFilter::NoResolvedReaders,
        Some(&unresolved)
    ));
    assert!(override_filter_matches(
        OverrideFilter::PartiallyDecoded,
        Some(&partial)
    ));
    assert!(!override_filter_matches(
        OverrideFilter::NoResolvedReaders,
        Some(&partial)
    ));
}

#[test]
fn rendered_definition_identifiers_are_filterable() {
    let definition = UnlockDefinition {
        hash: 0x1304_C3FA,
        code: 0x0202,
        compact_slot: Some(502),
        name: Some("Sweet Business Acquired".into()),
        description: None,
        tested_by: vec![ProgressionContextDef {
            hash: 7,
            kind: ProgressionContextKind::Activity,
            name: "The Shattered Throne".into(),
            type_name: String::new(),
            description: String::new(),
            paths: Vec::new(),
            condition_programs: Vec::new(),
        }],
    };

    assert!(definition_matches("#12913", 12_913, &definition));
    assert!(definition_matches("0x1304c3fa", 12_913, &definition));
    assert!(definition_matches("1304c3fa", 12_913, &definition));
    assert!(definition_matches("sweet business", 12_913, &definition));
    assert!(definition_matches("shattered throne", 12_913, &definition));
    assert!(!definition_matches("0xdeadbeef", 12_913, &definition));
}

#[test]
fn objective_summary_includes_goal_hierarchy_and_limit_semantics() {
    let mut objective = ObjectiveDef {
        hash: 0x14D6_FB47,
        description: "Arc".into(),
        completion_value: 5_000,
        allow_overcompletion: true,
        allow_value_change_when_completed: true,
        owners: vec![ObjectiveOwnerDef {
            hash: 0x5A33_50CC,
            kind: ObjectiveOwnerKind::Metric,
            name: "Arc Final Blows".into(),
            type_name: "Metric".into(),
            description: "Arc metric".into(),
            traits: vec![
                ObjectiveOwnerTraitDef {
                    hash: 0x557C_63B3,
                    name: "All".into(),
                    description: String::new(),
                },
                ObjectiveOwnerTraitDef {
                    hash: 0x84EC_E10B,
                    name: "Seasonal".into(),
                    description: "Seasonal metric".into(),
                },
            ],
            paths: vec![vec!["Account".into(), "Metrics".into()]],
        }],
        ..ObjectiveDef::default()
    };

    assert_eq!(objective_goal_text(&objective), "Arc Final Blows: Arc");
    assert_eq!(
        objective_traits_text(&objective).as_deref(),
        Some("All, Seasonal")
    );
    assert_eq!(
        objective_hierarchy_paths(&objective),
        vec![vec!["Metrics".to_owned(), "Account".to_owned()]]
    );
    assert_eq!(objective_target_text(&objective), "≥5000");
    assert!(objective_target_tooltip(&objective).contains("Over-completion: allowed"));
    assert!(objective_matches("account", &objective));
    assert!(objective_matches("metric", &objective));
    assert!(objective_matches("seasonal", &objective));
    assert!(objective_matches("84ece10b", &objective));
    let details_tooltip = objective_details_tooltip(&objective);
    assert!(!details_tooltip.contains("Seasonal"));
    let traits_tooltip = objective_traits_tooltip(&objective);
    assert!(traits_tooltip.contains("All: 0x557C63B3"));
    assert!(traits_tooltip.contains("Seasonal: 0x84ECE10B"));

    objective.allow_overcompletion = false;
    assert_eq!(objective_target_text(&objective), "5000 max");
    assert!(objective_target_tooltip(&objective).contains("Over-completion: not allowed"));
    assert!(objective_matches("capped", &objective));

    objective.is_counting_downward = true;
    assert_eq!(objective_target_text(&objective), "5000 min");
    objective.allow_overcompletion = true;
    assert_eq!(objective_target_text(&objective), "≤5000");

    objective.description.clear();
    objective.owners[0].name.clear();
    objective.owners[0].type_name = "General inventory".into();
    assert_eq!(objective_goal_text(&objective), "0x14D6FB47");
}

#[test]
fn unnamed_objectives_use_real_reverse_context_without_claiming_ownership() {
    let objective = ObjectiveDef::default();
    let definition = UnlockDefinition {
        hash: 0xE2C6_8308,
        code: 1,
        compact_slot: Some(5_662),
        name: None,
        description: None,
        tested_by: vec![
            ProgressionContextDef {
                hash: 0,
                kind: ProgressionContextKind::Objective,
                name: String::new(),
                type_name: String::new(),
                description: String::new(),
                paths: Vec::new(),
                condition_programs: Vec::new(),
            },
            ProgressionContextDef {
                hash: 0xAABB_CCDD,
                kind: ProgressionContextKind::PresentationNode,
                name: "Menagerie".into(),
                type_name: String::new(),
                description: String::new(),
                paths: vec![vec![
                    "Minor".into(),
                    "Destinations".into(),
                    "Triumphs".into(),
                ]],
                condition_programs: Vec::new(),
            },
        ],
    };

    assert_eq!(
        objective_table_text(&objective, Some(&definition)),
        "Menagerie · objective 0x00000000"
    );
    assert_eq!(
        definition_hierarchy_paths(&definition),
        vec![vec![
            "Triumphs".to_owned(),
            "Destinations".to_owned(),
            "Minor".to_owned(),
        ]]
    );
}

#[test]
fn unnamed_objectives_surface_their_unlock_definition_reference() {
    let objective = ObjectiveDef {
        hash: 0x1234_5678,
        related_unlock_value_definition_index: Some(1_630),
        ..ObjectiveDef::default()
    };
    let definition = UnlockDefinition {
        hash: 0x32DC_3113,
        code: 1,
        compact_slot: Some(404),
        name: None,
        description: None,
        tested_by: vec![ProgressionContextDef {
            hash: objective.hash,
            kind: ProgressionContextKind::Objective,
            name: String::new(),
            type_name: String::new(),
            description: String::new(),
            paths: Vec::new(),
            condition_programs: Vec::new(),
        }],
    };

    assert_eq!(
        objective_table_text(&objective, Some(&definition)),
        "Objective 0x12345678 · value definition #1630"
    );
    assert!(definition_hierarchy_paths(&definition).is_empty());
}

#[test]
fn metric_traits_distinguish_rows_with_the_same_package_name() {
    let objective = |trait_hash, trait_name: &str| ObjectiveDef {
        description: "Arc".into(),
        owners: vec![ObjectiveOwnerDef {
            hash: 1,
            kind: ObjectiveOwnerKind::Metric,
            name: "Arc Final Blows".into(),
            type_name: "Metric".into(),
            description: String::new(),
            traits: vec![ObjectiveOwnerTraitDef {
                hash: trait_hash,
                name: trait_name.into(),
                description: String::new(),
            }],
            paths: vec![vec!["Account".into(), "Metrics".into()]],
        }],
        ..ObjectiveDef::default()
    };

    let seasonal = objective(0x84EC_E10B, "Seasonal");
    let weekly = objective(0x8C79_925E, "Weekly");
    assert_eq!(objective_goal_text(&seasonal), "Arc Final Blows: Arc");
    assert_eq!(objective_goal_text(&weekly), "Arc Final Blows: Arc");
    assert_eq!(
        objective_traits_text(&seasonal).as_deref(),
        Some("Seasonal")
    );
    assert_eq!(objective_traits_text(&weekly).as_deref(), Some("Weekly"));
}

#[test]
fn hierarchy_normalizes_leaf_first_and_bare_paths_to_one_root_first_path() {
    let leaf_first = vec!["Destination".to_owned(), "Metrics".to_owned()];
    let bare = vec!["Destination".to_owned()];
    let repeated_root = vec![
        "Metrics".to_owned(),
        "Destination".to_owned(),
        "Metrics".to_owned(),
    ];

    let expected = vec!["Metrics".to_owned(), "Destination".to_owned()];
    assert_eq!(normalize_hierarchy_path(&leaf_first, "Metrics"), expected);
    assert_eq!(normalize_hierarchy_path(&bare, "Metrics"), expected);
    assert_eq!(
        normalize_hierarchy_path(&repeated_root, "Metrics"),
        expected
    );
}

#[test]
fn compact_override_table_sort_maps_to_semantic_columns() {
    assert_eq!(
        override_table_sort(TableSort::ascending(0), false),
        TableSort::ascending(0)
    );
    assert_eq!(
        override_table_sort(TableSort::ascending(1), false),
        TableSort::ascending(2)
    );
    assert_eq!(
        override_table_sort(TableSort::ascending(2), false),
        TableSort::ascending(3)
    );
    assert_eq!(
        override_table_sort(TableSort::ascending(3), true),
        TableSort::ascending(3)
    );
}

#[test]
fn override_meaning_uses_authored_names_then_exact_package_readers() {
    let context = |name: &str| ProgressionContextDef {
        hash: 1,
        kind: ProgressionContextKind::ActivityAvailability,
        name: name.into(),
        type_name: String::new(),
        description: String::new(),
        paths: Vec::new(),
        condition_programs: Vec::new(),
    };
    let mut definition = UnlockDefinition {
        hash: 2,
        code: 1,
        compact_slot: Some(3),
        name: Some("Authored package meaning".into()),
        description: None,
        tested_by: vec![context("The Menagerie")],
    };
    assert_eq!(override_meaning(&definition), "Authored package meaning");

    definition.name = None;
    assert_eq!(override_meaning(&definition), "The Menagerie");

    definition.tested_by.push(context("The Gauntlet"));
    assert_eq!(
        override_meaning(&definition),
        "The Gauntlet · The Menagerie"
    );

    definition.tested_by.push(context("The Mockery"));
    assert_eq!(
        override_meaning(&definition),
        "The Gauntlet · The Menagerie · +1 more"
    );

    definition.tested_by.clear();
    assert_eq!(override_meaning(&definition), "Reader not resolved");
}

#[test]
fn objectives_without_package_paths_do_not_get_synthetic_categories() {
    let mut objective = ObjectiveDef {
        owners: vec![ObjectiveOwnerDef {
            hash: 1,
            kind: ObjectiveOwnerKind::InventoryItem,
            name: "Ace of Spades Catalyst".into(),
            type_name: "General inventory".into(),
            description: String::new(),
            traits: Vec::new(),
            paths: Vec::new(),
        }],
        ..ObjectiveDef::default()
    };

    assert!(objective_hierarchy_paths(&objective).is_empty());

    objective.owners[0].name.clear();
    objective.owners[0].type_name = "Item".into();
    assert!(objective_hierarchy_paths(&objective).is_empty());
    assert_eq!(objective_goal_text(&objective), "Item: 0x00000000");
}

#[test]
fn context_paths_do_not_invent_an_uncategorized_parent() {
    assert_eq!(
        normalize_context_path(&["Werner 99-40".into()]),
        vec!["Werner 99-40"]
    );
    assert_eq!(
        normalize_context_path(&["Destination".into(), "Metrics".into()]),
        vec!["Metrics", "Destination"]
    );
}

#[test]
fn objective_hierarchy_lists_every_distinct_path_without_a_cap() {
    let paths = (0..300)
        .map(|index| vec![format!("Branch {index}"), "Metrics".into()])
        .collect::<Vec<_>>();
    let objective = ObjectiveDef {
        owners: vec![
            ObjectiveOwnerDef {
                hash: 1,
                kind: ObjectiveOwnerKind::Metric,
                name: "Metric".into(),
                type_name: "Metric".into(),
                description: String::new(),
                traits: Vec::new(),
                paths: paths.clone(),
            },
            ObjectiveOwnerDef {
                hash: 2,
                kind: ObjectiveOwnerKind::Record,
                name: "Record".into(),
                type_name: "Triumph / record".into(),
                description: String::new(),
                traits: Vec::new(),
                paths: vec![paths[0].clone(), vec!["Account".into(), "Triumphs".into()]],
            },
        ],
        ..ObjectiveDef::default()
    };

    let locations = objective_hierarchy_paths(&objective);

    assert_eq!(locations.len(), 301);
    assert_eq!(
        locations.first().unwrap(),
        &vec!["Metrics".to_owned(), "Branch 0".to_owned()]
    );
    assert_eq!(
        locations.get(299).unwrap(),
        &vec!["Metrics".to_owned(), "Branch 299".to_owned()]
    );
    assert_eq!(
        locations.last().unwrap(),
        &vec!["Triumphs".to_owned(), "Account".to_owned()]
    );
}

#[test]
fn objective_leaf_sort_is_stable_inside_a_branch() {
    let rows = [
        IndexedValue { index: 1, value: 5 },
        IndexedValue { index: 2, value: 4 },
        IndexedValue { index: 3, value: 3 },
    ];
    let objectives = [
        ObjectiveDef {
            description: "Alpha".into(),
            ..ObjectiveDef::default()
        },
        ObjectiveDef {
            description: "Alpha".into(),
            ..ObjectiveDef::default()
        },
        ObjectiveDef {
            description: "Beta".into(),
            ..ObjectiveDef::default()
        },
    ];
    let mut branch = ObjectiveHierarchyBranch::new("Metrics".into(), vec!["Metrics".into()]);
    branch.leaves = vec![
        ObjectiveHierarchyLeaf {
            row: &rows[0],
            definition_index: None,
            definition: None,
            objective_index: None,
            objective: Some(&objectives[0]),
        },
        ObjectiveHierarchyLeaf {
            row: &rows[1],
            definition_index: None,
            definition: None,
            objective_index: None,
            objective: Some(&objectives[1]),
        },
        ObjectiveHierarchyLeaf {
            row: &rows[2],
            definition_index: None,
            definition: None,
            objective_index: None,
            objective: Some(&objectives[2]),
        },
    ];

    let mut hierarchy = ObjectiveHierarchy {
        branches: vec![branch],
        leaves: Vec::new(),
    };
    sort_objective_hierarchy(&mut hierarchy, TableSort::ascending(0));

    assert_eq!(
        hierarchy.branches[0]
            .leaves
            .iter()
            .map(|leaf| leaf.row.index)
            .collect::<Vec<_>>(),
        vec![1, 2, 3]
    );
}

#[test]
fn filtered_hierarchy_auto_expands_matching_branches() {
    let row = IndexedValue { index: 1, value: 1 };
    let mut root = ObjectiveHierarchyBranch::new("Metrics".into(), vec!["Metrics".into()]);
    let mut child = ObjectiveHierarchyBranch::new(
        "Destination".into(),
        vec!["Metrics".into(), "Destination".into()],
    );
    child.leaves.push(ObjectiveHierarchyLeaf {
        row: &row,
        definition_index: None,
        definition: None,
        objective_index: None,
        objective: None,
    });
    root.children.push(child);
    let hierarchy = ObjectiveHierarchy {
        branches: vec![root],
        leaves: Vec::new(),
    };
    let state = UiState::default();

    assert_eq!(
        objective_matrix_lines(&hierarchy, "test", &state, false).len(),
        2
    );
    assert_eq!(
        objective_matrix_lines(&hierarchy, "test", &state, true).len(),
        3
    );
}

#[test]
fn tested_by_rows_have_no_cap_and_merge_identical_visible_contexts() {
    let mut contexts = (0..300)
        .map(|index| ProgressionContextDef {
            hash: index,
            kind: ProgressionContextKind::Activity,
            name: format!("Activity {index}"),
            type_name: String::new(),
            description: String::new(),
            paths: Vec::new(),
            condition_programs: Vec::new(),
        })
        .collect::<Vec<_>>();
    contexts.push(ProgressionContextDef {
        hash: 999,
        kind: ProgressionContextKind::ActivityAvailability,
        name: "Activity 0".into(),
        type_name: String::new(),
        description: String::new(),
        paths: Vec::new(),
        condition_programs: Vec::new(),
    });
    let definition = UnlockDefinition {
        tested_by: contexts,
        ..UnlockDefinition::default()
    };

    let lines = definition_context_lines(&definition);
    let display_lines = definition_context_display_lines(1, |_| Some((0, &definition)), false);

    assert_eq!(lines.len(), 300);
    assert_eq!(display_lines.len(), 300);
    assert_eq!(
        lines
            .iter()
            .find(|line| line.text() == "Activity 0")
            .unwrap()
            .contexts
            .len(),
        2
    );
}

#[test]
fn tested_by_rows_hide_empty_internal_refs_and_generic_inventory_buckets() {
    let definition = UnlockDefinition {
        tested_by: vec![
            ProgressionContextDef {
                hash: 1,
                kind: ProgressionContextKind::ExpressionMapping,
                name: String::new(),
                type_name: String::new(),
                description: String::new(),
                paths: Vec::new(),
                condition_programs: Vec::new(),
            },
            ProgressionContextDef {
                hash: 2,
                kind: ProgressionContextKind::InventoryItem,
                name: String::new(),
                type_name: "General inventory".into(),
                description: String::new(),
                paths: Vec::new(),
                condition_programs: Vec::new(),
            },
        ],
        ..UnlockDefinition::default()
    };

    let lines = definition_context_lines(&definition);
    assert!(lines.is_empty());
}
