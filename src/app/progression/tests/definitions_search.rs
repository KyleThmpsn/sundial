use super::*;

#[test]
fn progression_tables_include_package_definitions_without_authored_values() {
    let definitions = [
        ProgressionDefinition {
            definition_index: 3,
            hash: 0x1111_1111,
            scope: ProgressionScope::Account,
            scope_slot: Some(0),
            repeat_last_step: false,
            level_value: Some(0),
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
            level_value: Some(0),
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
            level_value: Some(0),
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
fn progression_target_sums_rank_costs_instead_of_taking_the_largest_step() {
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

    assert_eq!(progression_target(&definition), Some(400));
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
fn override_coverage_filters_distinguish_mapping_and_decode_confidence() {
    let unresolved = UnlockDefinition {
        hash: 1,
        code: 0,
        compact_slot: None,
        name: None,
        description: None,
        runtime_writers: Vec::new(),
        tested_by: Vec::new(),
    };
    let partial = UnlockDefinition {
        runtime_writers: Vec::new(),
        tested_by: vec![ProgressionContextDef {
            direct_references: Vec::new(),
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
        runtime_writers: Vec::new(),
        tested_by: vec![ProgressionContextDef {
            direct_references: Vec::new(),
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
