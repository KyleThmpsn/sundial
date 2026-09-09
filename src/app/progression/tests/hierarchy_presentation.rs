use super::*;

fn assert_objective_summary_context(objective: &ObjectiveDef) {
    assert_eq!(objective_goal_text(objective), "Arc Final Blows: Arc");
    assert_eq!(
        objective_traits_text(objective).as_deref(),
        Some("All, Seasonal")
    );
    assert_eq!(
        objective_hierarchy_paths(objective),
        vec![vec!["Metrics".to_owned(), "Account".to_owned()]]
    );
    assert_eq!(objective_target_text(objective), "≥5000");
    assert!(objective_matches("account", objective));
    assert!(objective_matches("metric", objective));
    assert!(objective_matches("seasonal", objective));
    assert!(objective_matches("84ece10b", objective));
    let details_tooltip = objective_details_tooltip(objective);
    assert!(!details_tooltip.contains("Seasonal"));
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

    assert_objective_summary_context(&objective);

    objective.allow_overcompletion = false;
    assert_eq!(objective_target_text(&objective), "5000 max");
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
        runtime_writers: Vec::new(),
        tested_by: vec![
            ProgressionContextDef {
                direct_references: Vec::new(),
                hash: 0,
                kind: ProgressionContextKind::Objective,
                name: String::new(),
                type_name: String::new(),
                description: String::new(),
                paths: Vec::new(),
                condition_programs: Vec::new(),
            },
            ProgressionContextDef {
                direct_references: Vec::new(),
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
        runtime_writers: Vec::new(),
        tested_by: vec![ProgressionContextDef {
            direct_references: Vec::new(),
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
        direct_references: Vec::new(),
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
        runtime_writers: Vec::new(),
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
    assert_eq!(override_meaning(&definition), "No Known References");
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
            direct_references: Vec::new(),
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
        direct_references: Vec::new(),
        hash: 999,
        kind: ProgressionContextKind::ActivityAvailability,
        name: "Activity 0".into(),
        type_name: String::new(),
        description: String::new(),
        paths: Vec::new(),
        condition_programs: Vec::new(),
    });
    let definition = UnlockDefinition {
        runtime_writers: Vec::new(),
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
        runtime_writers: Vec::new(),
        tested_by: vec![
            ProgressionContextDef {
                direct_references: Vec::new(),
                hash: 1,
                kind: ProgressionContextKind::ExpressionMapping,
                name: String::new(),
                type_name: String::new(),
                description: String::new(),
                paths: Vec::new(),
                condition_programs: Vec::new(),
            },
            ProgressionContextDef {
                direct_references: Vec::new(),
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
